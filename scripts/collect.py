#!/usr/bin/env python3
"""Scripted Hatari sessions for RAM-diffing "Disc" (Loriciel, 1990, Atari ST).

Runs a scenario file: boot the game, reach a one-player match, inject
frame-timed joystick input, and savebin $0-$8000 (where the game keeps its
state, per its use of 68000 short absolute addressing) at tagged moments.

Design notes (all of these were established empirically -- see KNOWN_ISSUES.md):

* Control channel is Hatari's --control-socket, not --cmd-fifo: a socket write
  fails loudly when Hatari dies, and Hatari's stdout+stderr (where every
  debugger reply goes) is then ours to capture from the subprocess.
* Debugger replies are bracketed with 'evaluate #<n>' markers rather than the
  obvious 'echo' -- Hatari 2.6.1's echo aborts on an assertion.
* Timing is in PAL VBLs, read back with 'evaluate VBL'.
* ST input arrives two different ways and the difference matters:
    - control socket 'hatari-event key*' injects ST *keyboard* scancodes.
      Good for the title screen's SPACE, useless for the joystick.
    - the joystick is emulated at the SDL layer from host keys, so joystick
      input has to be real X key events; we run Hatari on an Xvfb display and
      inject with XTEST.  This is also what makes the pipeline headless.
"""
import argparse, atexit, hashlib, json, os, re, signal, socket, struct, subprocess, sys, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEF_DISK = os.path.join(ROOT, "Disc (1990)(Loriciel)[cr Exo-7].st")
DEF_TOS = os.path.join(ROOT, "emutos-512k-1.4", "etos512us.img")

# Game state lives below $8000 (the code uses 68000 short absolute
# addressing), but a scenario can widen this to check that assumption.
STATE_LO, STATE_HI = 0x0, 0x8000

# ST keyboard scancodes, for the control socket (keyboard only, not joystick).
SCANCODE = {"Space": 57, "Return": 28, "Escape": 1,
            "Up": 72, "Down": 80, "Left": 75, "Right": 77}

# Host X keysyms that hatari.cfg [Joystick1] maps to joystick port 1
# (kUp/kDown/kLeft/kRight = cursor keys, kFire = Right Ctrl).
JOYKEY = {"Up": "Up", "Down": "Down", "Left": "Left", "Right": "Right",
          "Fire": "Control_R"}

# Screen signatures, sampled from Hatari's 832x552 BMP screenshots.
#
# MENU  ("ONE PLAYER" / "TWO PLAYERS"): the buttons are filled with (68,0,136)
#   dark purple on row 431, and the sky above y=200 is bare starfield.  The
#   purple row alone is NOT enough -- the character-select screen has a purple
#   banner on the same row -- hence the second, "empty sky" test.
# MATCH: the shield gauge under the left player's name draws two solid bright
#   horizontal rules across x=105..235, on rows 81 and 93.  Nothing else in
#   the game does.
MENU_ROW = 431
MENU_XS = range(270, 572, 4)
# The TRAINING / CHALLENGE / TOURNAMENT / CHAMPIONSHIP row is the only screen
# with a purple button at BOTH extremes of MENU_ROW: the two-button main menu
# and the single-banner chooser screens are centred.
MODE_ROW_LEFT, MODE_ROW_RIGHT = range(110, 255, 3), range(580, 725, 3)
# Frames of "Right" needed to slide the pointer from TRAINING onto each mode
# (the pointer starts at window x=240 and moves ~2.67 px/frame).
MODE_NUDGE = {"training": 0, "challenge": 36, "tournament": 94, "championship": 153}
SKY_YS, SKY_XS = range(60, 200, 4), range(150, 684, 4)
BAR_ROWS, BAR_XS = (81, 93), range(105, 235, 5)


class Timeout(RuntimeError):
    pass


def _bmp_sampler(path):
    """Return px(x, y) for a 24-bit BMP -- Hatari's screenshot format."""
    d = open(path, "rb").read()
    off = struct.unpack_from("<I", d, 10)[0]
    w, h = struct.unpack_from("<ii", d, 18)
    stride = ((w * 3) + 3) & ~3

    def px(x, y):
        i = off + (h - 1 - y) * stride + x * 3
        return (d[i + 2], d[i + 1], d[i])
    return w, h, px


class Xvfb:
    """Headless X display, so XTEST can deliver real key events to Hatari."""

    def __init__(self, size="1024x768x24"):
        self.size, self.proc, self.num = size, None, None

    def start(self):
        for num in range(90, 120):
            if os.path.exists("/tmp/.X%d-lock" % num):
                continue
            self.num = num
            break
        else:
            raise RuntimeError("no free X display number in :90-:119")
        self.proc = subprocess.Popen(
            ["Xvfb", ":%d" % self.num, "-screen", "0", self.size, "-nolisten", "tcp"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.time() + 10
        while time.time() < deadline:
            if os.path.exists("/tmp/.X11-unix/X%d" % self.num):
                time.sleep(0.3)
                return ":%d" % self.num
            if self.proc.poll() is not None:
                raise RuntimeError("Xvfb died on startup")
            time.sleep(0.1)
        raise Timeout("Xvfb :%d never came up" % self.num)

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(3)
            except subprocess.TimeoutExpired:
                self.proc.kill()


class Joypad:
    """Joystick-1 input for Hatari, as real X key events via XTEST."""

    def __init__(self, dispname):
        from Xlib import display, X, XK
        from Xlib.ext import xtest
        self.X, self.XK, self.xtest = X, XK, xtest
        self.d = display.Display(dispname)
        self.root = self.d.screen().root
        self.win = None
        self.down = set()

    def attach(self, timeout=15):
        """Find Hatari's window and give it X input focus (no WM on Xvfb)."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            for w in self.root.query_tree().children:
                try:
                    if (w.get_wm_name() or "").startswith("Hatari"):
                        self.win = w
                        # Cycle focus so SDL actually sees a FocusIn.
                        self.d.set_input_focus(self.X.PointerRoot,
                                               self.X.RevertToPointerRoot,
                                               self.X.CurrentTime)
                        self.d.sync()
                        self.d.set_input_focus(w, self.X.RevertToParent,
                                               self.X.CurrentTime)
                        g = w.get_geometry()
                        tr = w.translate_coords(self.root, 0, 0)
                        self.origin = (-tr.x, -tr.y)
                        self.size = (g.width, g.height)
                        self.warp(g.width // 2, g.height // 2)
                        self.d.sync()
                        return self
                except Exception:
                    continue
            time.sleep(0.2)
        raise Timeout("no Hatari window appeared on the Xvfb display")

    def _kc(self, keysym_name):
        return self.d.keysym_to_keycode(self.XK.string_to_keysym(keysym_name))

    def keydown(self, key):
        self.xtest.fake_input(self.d, self.X.KeyPress, self._kc(JOYKEY[key]))
        self.d.sync()
        self.down.add(key)

    def keyup(self, key):
        self.xtest.fake_input(self.d, self.X.KeyRelease, self._kc(JOYKEY[key]))
        self.d.sync()
        self.down.discard(key)

    def release_all(self):
        for key in list(self.down):
            self.keyup(key)

    def warp(self, x, y):
        """Warp the host pointer to window-relative (x, y)."""
        ox, oy = self.origin
        self.xtest.fake_input(self.d, self.X.MotionNotify, x=ox + x, y=oy + y)
        self.d.sync()

    def click(self, button=1):
        self.xtest.fake_input(self.d, self.X.ButtonPress, button)
        self.d.sync()
        time.sleep(0.05)
        self.xtest.fake_input(self.d, self.X.ButtonRelease, button)
        self.d.sync()

    def close(self):
        try:
            self.release_all()
            self.d.close()
        except Exception:
            pass


class Hatari:
    def __init__(self, disk=DEF_DISK, tos=DEF_TOS, logpath="tmp/hatari.log",
                 shotdir="tmp/shots", keep_window=False, verbose=False):
        self.disk, self.tos = disk, tos
        self.logpath, self.shotdir = logpath, shotdir
        self.keep_window, self.verbose = keep_window, verbose
        self.proc = self.sock = self.logf = self._logfd = None
        self.xvfb = self.pad = None
        self._marker = 0
        self._shotseq = 0

    # ---- lifecycle -------------------------------------------------------
    def start(self):
        os.makedirs(os.path.dirname(self.logpath) or ".", exist_ok=True)
        os.makedirs(self.shotdir, exist_ok=True)
        env = dict(os.environ)
        if not self.keep_window:
            self.xvfb = Xvfb()
            env["DISPLAY"] = self.xvfb.start()
            env["SDL_VIDEODRIVER"] = "x11"
            env.pop("WAYLAND_DISPLAY", None)
        elif env.get("XDG_SESSION_TYPE") == "wayland":
            # XTEST only works through X; force XWayland for the visible window.
            env["SDL_VIDEODRIVER"] = "x11"

        sockpath = "/tmp/hatari-collect-%d.socket" % os.getpid()
        if os.path.exists(sockpath):
            os.unlink(sockpath)
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        srv.bind(sockpath)
        srv.listen(1)
        self.logf = open(self.logpath, "wb")
        # stdbuf: Hatari sends debugger command output to stdout and its "> cmd"
        # echo to stderr.  Piped, stdout is block-buffered, so the two streams
        # interleave out of order and reply markers arrive late -- line-buffer
        # both so the merged log is in true order.
        args = ["stdbuf", "-oL", "-eL",
                "hatari", "-c", os.path.join(ROOT, "hatari.cfg"),
                "--control-socket", sockpath,
                "--tos", self.tos, "--disk-a", self.disk,
                "--protect-floppy", "on",     # the game writes to the image otherwise
                "--machine", "st", "--memsize", "1", "--sound", "off",
                "--joy1", "keys",   # nJoystickMode: 1 is a REAL stick, 2 is keys
                "--fastfdc", "on", "--window", "--zoom", "1", "--statusbar", "off",
                "--screenshot-dir", self.shotdir, "--screenshot-format", "bmp"]
        if self.verbose:
            print("RUN:", " ".join(args), file=sys.stderr)
        self.proc = subprocess.Popen(args, stdout=self.logf, stderr=subprocess.STDOUT,
                                     stdin=subprocess.DEVNULL, start_new_session=True,
                                     env=env)
        atexit.register(self.stop)
        srv.settimeout(20)
        try:
            self.sock, _ = srv.accept()
        except socket.timeout:
            self.stop()
            raise Timeout("Hatari never connected to %s; see %s" % (sockpath, self.logpath))
        finally:
            srv.close()
            os.unlink(sockpath)
        self._logfd = open(self.logpath, "r", errors="replace")
        self.pad = Joypad(env.get("DISPLAY", os.environ.get("DISPLAY"))).attach()
        return self

    def stop(self):
        if self.pad:
            self.pad.close()
            self.pad = None
        if self.proc and self.proc.poll() is None:
            try:
                os.killpg(os.getpgid(self.proc.pid), signal.SIGTERM)
                self.proc.wait(5)
            except Exception:
                try:
                    os.killpg(os.getpgid(self.proc.pid), signal.SIGKILL)
                except Exception:
                    pass
        for f in (self.sock, self._logfd, self.logf):
            try:
                f and f.close()
            except Exception:
                pass
        self.proc = self.sock = None
        if self.xvfb:
            self.xvfb.stop()
            self.xvfb = None

    def alive(self):
        return self.proc is not None and self.proc.poll() is None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.stop()

    # ---- control channel -------------------------------------------------
    def cmd(self, line):
        if not self.alive():
            raise RuntimeError("Hatari is not running (exit %s); see %s"
                               % (self.proc and self.proc.returncode, self.logpath))
        if self.verbose:
            print("->", line, file=sys.stderr)
        self.sock.sendall((line + "\n").encode())

    def dbg(self, cmd):
        self.cmd("hatari-debug " + cmd)

    def dbg_capture(self, cmd, timeout=10.0):
        self._marker += 1
        nb, ne = 900000 + 2 * self._marker, 900001 + 2 * self._marker
        beg, end = "#%d (dec)" % nb, "#%d (dec)" % ne
        self._logfd.read()
        self.dbg("evaluate #%d" % nb)
        self.dbg(cmd)
        self.dbg("evaluate #%d" % ne)
        buf, start = "", time.time()
        resent = False
        while time.time() - start < timeout:
            buf += self._logfd.read()
            if end in buf:
                return buf.split(beg, 1)[-1].split(end, 1)[0]
            if not self.alive():
                raise RuntimeError("Hatari died running %r; see %s" % (cmd, self.logpath))
            if not resent and time.time() - start > timeout / 2:
                # a long debugger output can page and swallow the next input
                # line, taking our end marker with it -- ask again once
                self.dbg("evaluate #%d" % ne)
                resent = True
            time.sleep(0.002)
        raise Timeout("no reply to %r in %.1fs; see %s" % (cmd, timeout, self.logpath))

    # ---- emulation state -------------------------------------------------
    _DEC = re.compile(r"#(-?\d+) \(dec\)")

    def vbl(self):
        out = self.dbg_capture("evaluate VBL")
        m = self._DEC.search(out)
        if not m:
            raise RuntimeError("cannot parse VBL from %r" % out)
        return int(m.group(1))

    def wait_frames(self, n):
        """Advance n PAL VBLs; lands within ~1 frame.  Returns the actual VBL."""
        target = self.vbl() + n
        while True:
            v = self.vbl()
            if v >= target:
                return v
            time.sleep(min(0.05, max(0.0, (target - v) * 0.01)))

    def fast_forward(self, on):
        self.cmd("hatari-option --fast-forward %s" % ("on" if on else "off"))

    def statesave(self, path):
        path = os.path.abspath(path)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        if os.path.exists(path):
            os.unlink(path)   # else statesave prompts "overwrite?" on dead stdin
        out = self.dbg_capture("statesave " + path, timeout=30)
        if not os.path.exists(path):
            raise RuntimeError("statesave failed: %r" % out.strip())

    def stateload(self, path):
        path = os.path.abspath(path)
        if not os.path.exists(path):
            raise FileNotFoundError(path)
        self.dbg_capture("stateload " + path, timeout=30)

    # ---- screen ----------------------------------------------------------
    def screen(self, name=None):
        """Grab a BMP screenshot; return (path, px(x, y))."""
        self._shotseq += 1
        path = os.path.abspath(os.path.join(
            self.shotdir, name or "s%04d.bmp" % self._shotseq))
        if os.path.exists(path):
            os.unlink(path)
        out = self.dbg_capture("screenshot " + path)
        if not os.path.exists(path):
            raise RuntimeError("screenshot failed: %r" % out.strip())
        return path, _bmp_sampler(path)[2]

    def on_menu(self, px=None):
        if px is None:
            _, px = self.screen("probe.bmp")
        purple = sum(1 for x in MENU_XS
                     if px(x, MENU_ROW)[2] > 100 and px(x, MENU_ROW)[0] < 120)
        sky = sum(1 for y in SKY_YS for x in SKY_XS if sum(px(x, y)) > 250)
        return purple >= 25 and sky < 100

    def screen_is_dark(self, px=None):
        """True while the game is loading from the floppy (screen blanked)."""
        if px is None:
            _, px = self.screen("probe.bmp")
        return sum(1 for y in SKY_YS for x in SKY_XS if sum(px(x, y)) > 60) < 20

    def on_mode_row(self, px=None):
        if px is None:
            _, px = self.screen("probe.bmp")
        hot = lambda xs: sum(1 for x in xs if px(x, MENU_ROW)[2] > 100
                             and px(x, MENU_ROW)[0] < 120)
        return hot(MODE_ROW_LEFT) >= 15 and hot(MODE_ROW_RIGHT) >= 15

    def in_match(self, px=None):
        if px is None:
            _, px = self.screen("probe.bmp")
        return all(sum(1 for x in BAR_XS if sum(px(x, y)) > 350) >= 24
                   for y in BAR_ROWS)

    # ---- input -----------------------------------------------------------
    def stkey(self, key, frames=4):
        """Tap an ST *keyboard* key through the control socket."""
        self.cmd("hatari-event keydown %d" % SCANCODE[key])
        self.wait_frames(frames)
        self.cmd("hatari-event keyup %d" % SCANCODE[key])

    def hold(self, key, frames):
        """Hold a joystick direction/fire for n frames.  Returns the end VBL."""
        self.pad.keydown(key)
        try:
            return self.wait_frames(frames)
        finally:
            self.pad.keyup(key)

    def release(self, key=None):
        if key is None:
            self.pad.release_all()
        else:
            self.pad.keyup(key)

    # ---- capture ---------------------------------------------------------
    def dump(self, path, lo=STATE_LO, hi=STATE_HI):
        path = os.path.abspath(path)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        if os.path.exists(path):
            os.unlink(path)
        n = hi - lo
        vbl = self.vbl()
        out = self.dbg_capture("savebin %s $%x $%x" % (path, lo, n))
        if "Wrote" not in out:
            raise RuntimeError("savebin failed: %r" % out.strip())
        deadline = time.time() + 5
        while time.time() < deadline:
            if os.path.exists(path) and os.path.getsize(path) == n:
                return vbl
            time.sleep(0.02)
        raise Timeout("dump %s never reached %d bytes" % (path, n))

    # ---- navigation ------------------------------------------------------
    def navigate_to_match(self, boot_frames=12000, load_frames=600,
                          space_tries=60, mode="training", verbose=True):
        """Title screen -> ONE PLAYER -> first character -> TRAINING -> match.

        The discovered flow (screenshots in tmp/shots16/ of the discovery run):
          title  --SPACE (ST keyboard)--> ONE PLAYER / TWO PLAYERS menu
          menu   --joy1 fire-----------> "SELECT THE FIRST PLAYER" (8 faces)
          faces  --joy1 fire-----------> 16 faces + the mode row
          mode   --joy1 fire-----------> the match (CHALLENGE and up first
                                         ask for an opponent)

        `mode` is one of MODE_NUDGE: TRAINING is a timed warm-up in which the
        disc is never served, CHALLENGE is a real bout.
        """
        mode_nudge = MODE_NUDGE[mode] if isinstance(mode, str) else int(mode)
        self.fast_forward(True)
        self.wait_frames(boot_frames)
        for i in range(space_tries):
            # SPACE only registers once the title screen has finished loading,
            # so keep tapping until the menu shows up.
            self.stkey("Space", 4)
            self.wait_frames(600)
            if self.on_menu():
                if verbose:
                    print("[nav] menu reached after %d space taps (VBL %d)"
                          % (i + 1, self.vbl()), file=sys.stderr)
                break
        else:
            raise Timeout("never reached the ONE PLAYER menu after %d space taps"
                          % space_tries)
        # The pointer starts on ONE PLAYER's right edge; nudge it to the centre.
        self.hold("Left", 20)
        # Fire takes whatever the pointer is on, and every chooser defaults to
        # its leftmost item.  Rather than counting screens (TRAINING needs two
        # more fires, CHALLENGE three -- it also asks for the opponent), look
        # at what is on screen each time and only aim on the mode row.
        nudged = False
        fires = 0
        for step in range(40):
            path, px = self.screen("nav%02d.bmp" % step)
            if self.screen_is_dark(px):
                # floppy load in progress -- firing here would be swallowed, or
                # worse, land in the middle of the match that is loading.
                self.wait_frames(load_frames)
                continue
            if self.in_match(px):
                if verbose:
                    print("[nav] match live after %d fire(s) (VBL %d)"
                          % (fires, self.vbl()), file=sys.stderr)
                self.fast_forward(False)
                return self
            on_modes = self.on_mode_row(px)
            if on_modes and mode_nudge and not nudged:
                self.hold("Right", mode_nudge)
                self.wait_frames(10)
                nudged = True
            if verbose:
                print("[nav] step %d: %s -> fire"
                      % (step, "mode row" if on_modes else "chooser"),
                      file=sys.stderr)
            self.hold("Fire", 10)
            fires += 1
            # Poll rather than sleeping out the whole load budget: a CHALLENGE
            # round is only a few hundred frames long, so the savestate has to
            # be taken as early into it as we can catch it.
            waited = 0
            while waited < load_frames:
                self.wait_frames(min(100, load_frames - waited))
                waited += 100
                if self.in_match():
                    break
        raise RuntimeError("never reached a match; last screenshot is in "
                           + self.shotdir)
        self.fast_forward(False)
        return self

    def enter_match(self, cache=None, fresh=False, **kw):
        """Reach a live match, via a cached savestate when there is one.

        Floppy loading makes a cold boot take ~40s even fast-forwarded, and a
        savestate additionally makes every scenario start from a bit-identical
        machine state -- which is what makes cross-scenario diffing meaningful.
        """
        if cache and os.path.exists(cache) and not fresh:
            self.stateload(cache)
            time.sleep(0.5)
            if self.in_match():
                print("[nav] resumed from %s" % cache, file=sys.stderr)
                self.fast_forward(False)
                return self
            print("[nav] %s is stale, booting from scratch" % cache, file=sys.stderr)
        self.navigate_to_match(**kw)
        if cache:
            self.statesave(cache)
        return self

    # ---- oracle seed capture ---------------------------------------------
    _REGLINE = re.compile(r"\b([DA][0-7]|USP|ISP)\s+([0-9A-Fa-f]{8})\b")
    _SR = re.compile(r"SR=([0-9A-Fa-f]{4})")

    def registers(self):
        """Parse `cpureg` into {name: value}.  SR carries the interrupt mask."""
        out = self.dbg_capture("r", timeout=20)
        regs = {k.upper(): int(v, 16) for k, v in self._REGLINE.findall(out)}
        m = self._SR.search(out)
        if not m:
            raise RuntimeError("cannot parse SR from %r" % out)
        regs["SR"] = int(m.group(1), 16)
        want = ["D%d" % i for i in range(8)] + ["A%d" % i for i in range(8)]
        missing = [k for k in want + ["USP", "ISP"] if k not in regs]
        if missing:
            raise RuntimeError("cpureg is missing %s in %r" % (missing, out))
        return regs

    def run_to_counter(self, target, timeout=60):
        """Advance until the game's own $6ab4 reaches `target`.

        Counter-addressed, not wall-clock: the frame a seed is minted at has to
        be the frame the differ validated, and wall time cannot promise that.
        """
        deadline = time.time() + timeout
        while time.time() < deadline:
            cur = self.peek_word(0x6ab4)
            if cur is None:
                raise RuntimeError("cannot read $6ab4")
            if cur >= target:
                return cur
            time.sleep(min(0.4, max(0.005, (target - cur) * 0.02 * 0.8)))
        raise Timeout("$6ab4 never reached %d" % target)

    def seed(self, path, lo=0x0, hi=0x100000, at_pc=None):
        """Capture a frame-boundary seed for the oracle.

        The capture has to happen exactly AT the sampling point (PC == the VBL
        handler entry, before its first instruction). Pausing with
        `hatari-stop` after seeing the breakpoint is not good enough -- the
        pause lands wherever the emulator happens to be some milliseconds
        later, which measured as SR=$2309/IM=3 instead of the $2404/IM=4 that
        holds at the handler entry.

        So let the breakpoint itself do the work: `:file` runs a debugger
        command script at the hit, with emulation still notionally at that
        instruction. The script saves RAM and dumps the registers; we parse
        both back out of the log.
        """
        pc = at_pc or self.VBL_HANDLER
        path = os.path.abspath(path)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        for stale in (path, path + ".json"):
            if os.path.exists(stale):
                os.unlink(stale)

        script = os.path.abspath(os.path.join(self.shotdir, "seed_cmds.txt"))
        with open(script, "w") as f:
            # RAM + registers + the MFP timer registers.  The oracle needs the
            # MFP shadow because a sound effect may already be running at the
            # seed instant: Timer A's stream cursor lives in USP (captured with
            # the registers) but the timer only ticks if TACR says so.
            f.write("savebin %s $%x $%x\nr\nm b $fffa19-$fffa22\n"
                    % (path, lo, hi - lo))

        mark = os.path.getsize(self.logpath)
        self.dbg_capture("b pc = $%x :once :trace :file %s" % (pc, script))
        deadline = time.time() + 15
        text = ""
        while time.time() < deadline:
            with open(self.logpath, "r", errors="replace") as f:
                f.seek(mark)
                text = f.read()
            if "Wrote" in text and self._SR.search(text):
                break
            if not self.alive():
                raise RuntimeError("Hatari died during seed capture")
            time.sleep(0.01)
        else:
            self.dbg_capture("b all")
            raise Timeout("no seed capture at $%x within 15s; is a match live?" % pc)

        regs = {k.upper(): int(v, 16) for k, v in self._REGLINE.findall(text)}
        regs["SR"] = int(self._SR.search(text).group(1), 16)
        mfp = {}
        for line in self._MEMLINE.finditer(text):
            a0 = int(line.group(1), 16) & 0xffffff
            for k, byte in enumerate(line.group(2).split()):
                mfp[a0 + k] = int(byte, 16)
        hw = {name: mfp.get(addr) for name, addr in
              (("TACR", 0xfffa19), ("TBCR", 0xfffa1b), ("TADR", 0xfffa1f),
               ("TBDR", 0xfffa21))}
        if any(v is None for v in hw.values()):
            raise RuntimeError("seed capture did not return the MFP registers")
        cur = re.search(r"^0*([0-9a-f]{4,8}) [0-9a-f]{4}", text, re.M)
        regs["PC"] = int(cur.group(1), 16) if cur else pc
        missing = [k for k in ["D%d" % i for i in range(8)]
                   + ["A%d" % i for i in range(8)] + ["USP", "ISP"]
                   if k not in regs]
        if missing:
            raise RuntimeError("cpureg output is missing %s" % missing)
        if regs["PC"] != pc:
            raise RuntimeError("seed captured at PC $%x, expected $%x -- the "
                               "sampling-point contract is broken"
                               % (regs["PC"], pc))

        for _ in range(200):
            if os.path.exists(path) and os.path.getsize(path) == hi - lo:
                break
            time.sleep(0.02)
        else:
            raise Timeout("seed RAM image never reached %d bytes" % (hi - lo))

        blob = open(path, "rb").read()
        digest = hashlib.sha256(blob).hexdigest()
        # read $6ab4 out of the image itself, not with a live peek -- the
        # emulator has moved on by the time we get here
        cnt = None
        if lo <= 0x6ab4 and 0x6ab6 <= hi:
            cnt = (blob[0x6ab4 - lo] << 8) | blob[0x6ab5 - lo]
        # Record the decoded joystick byte: a seed frozen mid-press is a
        # legitimate machine state but a trap for any consumer replaying a
        # different input script from it.
        joy = blob[0x6c58 - lo] if lo <= 0x6c58 < hi else None
        meta = {"ram": os.path.basename(path), "lo": lo, "hi": hi,
                "sha256": digest, "registers": regs, "mfp": hw, "pc": pc,
                "joystick_6c58": joy,
                "vbl_counter_6ab4": cnt,
                "note": "captured by a :file action at PC == $%x, before that "
                        "instruction executed" % pc}
        with open(path + ".json", "w") as f:
            json.dump(meta, f, indent=1, sort_keys=True)
        return meta

    def peek_word(self, addr):
        out = self.dbg_capture("evaluate ($%x).w" % addr)
        m = self._DEC.search(out)
        return int(m.group(1)) & 0xffff if m else None

    # ---- per-frame memory tracing ----------------------------------------
    _MEMLINE = re.compile(r"^([0-9A-F]{8}): ((?:[0-9a-f]{2} ?)+)", re.M)
    _HIT = re.compile(r"^\d+\. CPU breakpoint condition\(s\) matched", re.M)

    VBL_HANDLER = 0x8198   # vector $70; its first instruction is addq.w #1,$6ab4

    def frame_trace(self, base, frames, settle=0.0, during=None, at=0.3,
                    at_pc=None):
        """Capture one memdump of `base`.. per PAL VBL, for `frames` frames.

        A savebin costs ~2 emulated frames (Hatari services the control socket
        once per frame), which makes per-frame sampling impossible from the
        outside -- and --slowdown does not help, because the cost is in frames,
        not wall time.  So let Hatari do the work instead: 'lock memdump <addr>'
        plus a breakpoint that trips on every VBL change writes a snapshot into
        the log by itself, with no round trip at all.

        Sampling point: PC == $8198 (VBL handler entry), *before* that
        instruction runs -- so $6ab4 still holds the previous frame's count.
        See reports/oracle-scope.md; the oracle must honour the same contract.

        Returns [(vbl, {addr: byte}), ...] in frame order.  The span is
        whatever nMemdumpLines gives -- 200 lines = 3200 bytes with the
        hatari.cfg shipped here.
        """
        self.dbg_capture("lock memdump $%x" % base)
        vbl0 = self.vbl()
        mark = os.path.getsize(self.logpath)
        # Break at the VBL handler's entry, not on Hatari's VBL variable.
        # Both usually coincide, but "VBL ! VBL" reports whatever instruction
        # happens to be executing when the variable ticks -- measured landing
        # inside the Timer A handler ($83fc) on 1 frame in 8.  The sampling
        # point has to be exact or the oracle differ chases phantoms.
        self.dbg_capture("b pc = $%x :trace :lock" % (at_pc or self.VBL_HANDLER))
        try:
            if settle:
                time.sleep(settle)
            total = frames * 0.02 * 1.15 + 0.3
            if during:
                # Fire the stimulus from inside the trace window; XTEST does
                # not go through the control socket, so it costs no frames.
                # A callable that owns the whole timeline (its own sleeps)
                # gets `at=None`; otherwise it is a one-shot at that fraction.
                if at is None:
                    during()
                    time.sleep(0.3)
                else:
                    time.sleep(total * at)
                    during()
                    time.sleep(total * (1 - at))
            else:
                time.sleep(total)
        finally:
            self.dbg_capture("b all", timeout=20)
            self.dbg_capture("lock default", timeout=20)
        with open(self.logpath, "r", errors="replace") as f:
            f.seek(mark)
            text = f.read()

        hits = list(self._HIT.finditer(text))
        snaps = []
        for i, m in enumerate(hits):
            chunk = text[m.end():hits[i + 1].start() if i + 1 < len(hits) else len(text)]
            mem = {}
            for line in self._MEMLINE.finditer(chunk):
                addr = int(line.group(1), 16)
                for k, byte in enumerate(line.group(2).split()):
                    mem[addr + k] = int(byte, 16)
            if mem:
                snaps.append(mem)
        # Label frames with the game's OWN counter when the window covers it:
        # $6ab4 is incremented by the very instruction we stop before, so it is
        # exact ground truth and lets the oracle differ align on something both
        # emulators compute rather than on an index we invented.  Otherwise
        # fall back to counting from the VBL read before arming.
        if all(a in snaps[0] for a in (0x6ab4, 0x6ab5)) if snaps else False:
            out = [((m[0x6ab4] << 8) | m[0x6ab5], m) for m in snaps]
            steps = {out[i + 1][0] - out[i][0] for i in range(len(out) - 1)}
            if steps - {1}:
                print("[frame_trace] WARNING: $6ab4 steps by %s, expected 1 -- "
                      "frames were dropped" % sorted(steps), file=sys.stderr)
            return out
        return [(vbl0 + 1 + i, mem) for i, mem in enumerate(snaps)]

    # ---- write-origin capture (phase 3) ----------------------------------
    def watch(self, addr, width="w"):
        """Arm a change-tracking breakpoint on addr.

        ':trace' keeps the emulation running -- stopping would drop Hatari into
        the debugger's readline loop, and we have no stdin to get it out again.
        ':lock' makes every hit print the machine state block, which carries
        the PC.  Note the reported CPU= is the instruction *after* the write,
        so treat it as "the writer is immediately before here".
        """
        self.dbg_capture("lock default")
        out = self.dbg_capture("b ($%x).%s ! ($%x).%s :trace :lock"
                               % (addr, width, addr, width))
        return os.path.getsize(self.logpath), out

    def unwatch(self, addr, mark):
        """Disarm and report where the writes came from."""
        self.dbg_capture("b all")
        with open(self.logpath, "r", errors="replace") as f:
            f.seek(mark)
            text = f.read()
        pcs = re.findall(r"CPU=\$([0-9a-fA-F]+)", text)
        hits = {}
        for pc in pcs:
            hits[pc.lower()] = hits.get(pc.lower(), 0) + 1
        info = {"addr": "$%x" % addr, "hits": len(pcs),
                "writer_pcs": sorted(hits, key=hits.get, reverse=True),
                "pc_counts": hits}
        for pc in list(info["writer_pcs"])[:3]:
            v = int(pc, 16)
            info.setdefault("disasm", {})[("$%s" % pc)] = self.dbg_capture(
                "disasm $%x $%x" % (max(0, v - 0x28), v + 0x8), timeout=15).strip()
        info["history"] = self.dbg_capture("history cpu", timeout=15).strip()
        self.dbg_capture("lock default")
        return info


# ---------------------------------------------------------------------------
# scenario runner
# ---------------------------------------------------------------------------

def load_scenario(path):
    text = open(path).read()
    try:
        import yaml
        return yaml.safe_load(text)
    except ImportError:
        return json.loads(text)   # scenario files are also valid JSON-ish


def run_scenario(scn, h, outdir):
    """Execute a scenario's step list; returns the dump metadata."""
    os.makedirs(outdir, exist_ok=True)
    lo = int(str(scn.get("range", [STATE_LO, STATE_HI])[0]), 0)
    hi = int(str(scn.get("range", [STATE_LO, STATE_HI])[1]), 0)
    meta = {"scenario": scn.get("name"), "base": lo, "top": hi, "dumps": []}
    settle = scn.get("settle", 100)
    pending = []
    if settle:
        h.wait_frames(settle)
    for n, step in enumerate(scn["steps"]):
        if "wait" in step:
            h.wait_frames(int(step["wait"]))
        elif "hold" in step:
            keys = step["hold"]
            keys = [keys] if isinstance(keys, str) else list(keys)
            frames = int(step.get("frames", 25))
            for k in keys[:-1]:
                h.pad.keydown(k)
            try:
                h.hold(keys[-1], frames)
            finally:
                for k in keys[:-1]:
                    h.pad.keyup(k)
        elif "release" in step:
            h.release()
        elif "dump" in step:
            tag = str(step["dump"])
            path = os.path.join(outdir, tag + ".bin")
            vbl = h.dump(path, lo, hi)
            live = h.in_match()
            meta["dumps"].append({"tag": tag, "vbl": vbl, "file": path,
                                  "in_match": live})
            print("[dump] %-4s VBL %d %s-> %s"
                  % (tag, vbl, "" if live else "(NOT in match!) ", path),
                  file=sys.stderr)
        elif "screenshot" in step:
            path, _ = h.screen(str(step["screenshot"]) + ".bmp")
            print("[shot] %s" % path, file=sys.stderr)
        elif "seed" in step:
            path = str(step["seed"])
            info = h.seed(path,
                          int(str(step.get("lo", "0x0")), 0),
                          int(str(step.get("hi", "0x100000")), 0))
            meta.setdefault("seeds", []).append(info)
            print("[seed]  %s  sha256=%s..  $6ab4=%s"
                  % (path, info["sha256"][:16], info["vbl_counter_6ab4"]),
                  file=sys.stderr)
        elif "trace" in step:
            name = str(step["trace"])
            base = int(str(step.get("base", "$6e3e")).lstrip("$"), 16)
            nf = int(step.get("frames", 60))
            snaps = h.frame_trace(base, nf)
            path = os.path.join(outdir, "trace_%s.json" % name)
            with open(path, "w") as f:
                json.dump([[v, {("%x" % k): b for k, b in m.items()}]
                           for v, m in snaps], f)
            meta.setdefault("traces", []).append(
                {"name": name, "base": "$%x" % base, "frames": len(snaps),
                 "file": path,
                 "vbl_range": [snaps[0][0], snaps[-1][0]] if snaps else None})
            print("[trace] %-6s %d frames from $%x -> %s"
                  % (name, len(snaps), base, path), file=sys.stderr)
        elif "watch" in step:
            addr = int(str(step["watch"]).lstrip("$"), 16)
            mark, armed = h.watch(addr, step.get("width", "w"))
            pending.append((addr, mark, armed))
        elif "unwatch" in step:
            for addr, mark, armed in pending:
                info = h.unwatch(addr, mark)
                info["armed"] = armed.strip()
                meta.setdefault("watches", []).append(info)
                print("[watch] $%x: %d hit(s) from PC %s"
                      % (addr, info["hits"], ", ".join("$" + p for p in
                                                       info["writer_pcs"][:3])),
                      file=sys.stderr)
            pending = []
        else:
            raise ValueError("step %d: unknown step %r" % (n, step))
        if not h.alive():
            raise RuntimeError("Hatari died during step %d (%r)" % (n, step))
    for addr, mark, armed in pending:      # scenario forgot to unwatch
        info = h.unwatch(addr, mark)
        info["armed"] = armed.strip()
        meta.setdefault("watches", []).append(info)
    return meta


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--scenario", required=True, help="scenario YAML/JSON file")
    ap.add_argument("--disk", default=DEF_DISK)
    ap.add_argument("--tos", default=DEF_TOS)
    ap.add_argument("--dumpdir", default=None,
                    help="default: dumps/<scenario name>")
    ap.add_argument("--keep-window", action="store_true",
                    help="show Hatari on the real display instead of Xvfb")
    ap.add_argument("--state", default=None,
                    help="savestate cache (default: tmp/match_<mode>.sav; "
                         "'' disables caching)")
    ap.add_argument("--mode", default=None,
                    help="match mode: " + "/".join(MODE_NUDGE)
                         + " (default: the scenario's, else training)")
    ap.add_argument("--fresh", action="store_true",
                    help="ignore the savestate cache and boot from the floppy")
    ap.add_argument("-v", "--verbose", action="store_true")
    a = ap.parse_args(argv)

    scn = load_scenario(a.scenario)
    name = scn.get("name") or os.path.splitext(os.path.basename(a.scenario))[0]
    mode = a.mode or scn.get("mode", "training")
    state = a.state if a.state is not None else "tmp/match_%s.sav" % mode
    outdir = a.dumpdir or os.path.join("dumps", name)
    h = Hatari(disk=a.disk, tos=a.tos, keep_window=a.keep_window,
               logpath="tmp/%s.log" % name, shotdir="tmp/shots-%s" % name,
               verbose=a.verbose)
    rc = 0
    try:
        h.start()
        h.enter_match(cache=state or None, fresh=a.fresh, mode=mode,
                      boot_frames=scn.get("boot_frames", 12000),
                      load_frames=scn.get("load_frames", 600))
        warmup = scn.get("warmup", 0)
        if warmup:
            h.fast_forward(True)
            h.wait_frames(warmup)
            h.fast_forward(False)
        meta = run_scenario(scn, h, outdir)
        meta["source"] = os.path.abspath(a.scenario)
        with open(os.path.join(outdir, "meta.json"), "w") as f:
            json.dump(meta, f, indent=2)
        print("wrote %d dumps to %s" % (len(meta["dumps"]), outdir))
    except KeyboardInterrupt:
        print("interrupted", file=sys.stderr)
        rc = 130
    except Exception as e:
        print("FAILED: %s: %s" % (type(e).__name__, e), file=sys.stderr)
        try:
            print("last screen: %s" % h.screen("failure.bmp")[0], file=sys.stderr)
        except Exception:
            pass
        rc = 1
    finally:
        h.stop()
    return rc


if __name__ == "__main__":
    sys.exit(main())

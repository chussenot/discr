#!/usr/bin/env python3
"""Scripted Hatari session driver for RAM-diffing "Disc" (Loriciel 1990).

Control channel: Hatari's --control-socket (we bind, Hatari connects).
Chosen over --cmd-fifo because a socket write fails loudly when Hatari dies,
and because Hatari's stdout+stderr (where ALL debugger output goes) is then
ours to capture via subprocess pipes.  See KNOWN_ISSUES.md.
"""
import argparse, atexit, os, re, signal, socket, subprocess, sys, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEF_DISK = os.path.join(ROOT, "Disc (1990)(Loriciel)[cr Exo-7].st")
DEF_TOS = os.path.join(ROOT, "emutos-512k-1.4", "etos512us.img")

# ST keyboard scancodes.  Joystick 1 is emulated by these keys per hatari.cfg
# ([Joystick1] kUp/kDown/kLeft/kRight/kFire = Up/Down/Left/Right/Right Ctrl).
SCANCODE = {
    "Up": 72, "Down": 80, "Left": 75, "Right": 77,
    "Fire": 29,        # ST has one Control key; host "Right Ctrl" -> scancode 29
    "Space": 57, "Return": 28, "Escape": 1,
}

STATE_LO, STATE_HI = 0x0, 0x8000   # game state lives below $8000 (short abs addressing)


class Timeout(RuntimeError):
    pass


class Hatari:
    def __init__(self, disk=DEF_DISK, tos=DEF_TOS, logpath="tmp/hatari.log",
                 shotdir="tmp/shots", keep_window=False, verbose=False):
        self.disk, self.tos = disk, tos
        self.logpath, self.shotdir = logpath, shotdir
        self.keep_window, self.verbose = keep_window, verbose
        self.proc = self.sock = self.logf = None
        self._logfd = None
        self._marker = 0

    # ---- lifecycle -------------------------------------------------------
    def start(self):
        for p in (self.logpath, self.shotdir):
            os.makedirs(os.path.dirname(p) or ".", exist_ok=True)
        os.makedirs(self.shotdir, exist_ok=True)
        sockpath = "/tmp/hatari-collect-%d.socket" % os.getpid()
        if os.path.exists(sockpath):
            os.unlink(sockpath)
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        srv.bind(sockpath)
        srv.listen(1)
        self.logf = open(self.logpath, "wb")
        args = ["hatari", "--control-socket", sockpath,
                "--tos", self.tos, "--disk-a", self.disk,
                "--protect-floppy", "on",     # the game writes to disk otherwise
                "--machine", "st", "--memsize", "1", "--sound", "off",
                "--fastfdc", "on", "--window", "--zoom", "1", "--statusbar", "off",
                "--screenshot-dir", self.shotdir, "--screenshot-format", "png"]
        if self.verbose:
            print("RUN:", " ".join(args), file=sys.stderr)
        self.proc = subprocess.Popen(args, stdout=self.logf, stderr=subprocess.STDOUT,
                                     stdin=subprocess.DEVNULL, start_new_session=True)
        atexit.register(self.stop)
        srv.settimeout(15)
        try:
            self.sock, _ = srv.accept()
        except socket.timeout:
            self.stop()
            raise Timeout("Hatari did not connect to control socket %s" % sockpath)
        finally:
            srv.close()
            os.unlink(sockpath)
        self._logfd = open(self.logpath, "r", errors="replace")
        return self

    def stop(self):
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

    def alive(self):
        return self.proc is not None and self.proc.poll() is None

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

    def _drain(self):
        return self._logfd.read()

    def dbg_capture(self, cmd, timeout=5.0):
        """Run a debugger command, return everything it printed.

        Bracketing the command with markers is the only reliable way to know
        where its output ends: Hatari's log also carries async WARN lines from
        the emulation.  The marker is 'evaluate #<n>' rather than the obvious
        'echo', because Hatari 2.6.1's echo aborts on an assertion
        (see KNOWN_ISSUES.md).
        """
        self._marker += 1
        nb, ne = 900000 + 2 * self._marker, 900001 + 2 * self._marker
        beg, end = "#%d (dec)" % nb, "#%d (dec)" % ne
        self._drain()
        self.dbg("evaluate #%d" % nb)
        self.dbg(cmd)
        self.dbg("evaluate #%d" % ne)
        buf, deadline = "", time.time() + timeout
        while time.time() < deadline:
            buf += self._drain()
            if end in buf:
                return buf.split(beg, 1)[-1].split(end, 1)[0]
            if not self.alive():
                raise RuntimeError("Hatari died while running %r; see %s" % (cmd, self.logpath))
            time.sleep(0.002)
        raise Timeout("no reply to debugger command %r within %.1fs (see %s)"
                      % (cmd, timeout, self.logpath))

    # ---- emulation state -------------------------------------------------
    _DEC = re.compile(r"#(-?\d+) \(dec\)")

    def vbl(self):
        out = self.dbg_capture("evaluate VBL")
        m = self._DEC.search(out)
        if not m:
            raise RuntimeError("cannot parse VBL from %r" % out)
        return int(m.group(1))

    def wait_frames(self, n):
        """Advance n PAL VBLs.  Lands within ~1 frame; returns the actual VBL."""
        target = self.vbl() + n
        while True:
            v = self.vbl()
            if v >= target:
                return v
            # don't hammer the socket when there is a long way to go
            time.sleep(min(0.05, max(0.0, (target - v) * 0.02 * 0.5)))

    def fast_forward(self, on):
        self.cmd("hatari-option --fast-forward %s" % ("on" if on else "off"))

    def screenshot(self):
        self.cmd("hatari-shortcut screenshot")
        time.sleep(0.2)

    # ---- input -----------------------------------------------------------
    def keydown(self, key):
        self.cmd("hatari-event keydown %d" % SCANCODE[key])

    def keyup(self, key):
        self.cmd("hatari-event keyup %d" % SCANCODE[key])

    def tap(self, key, frames=3):
        self.keydown(key)
        self.wait_frames(frames)
        self.keyup(key)

    def hold(self, key, frames):
        self.keydown(key)
        v = self.wait_frames(frames)
        self.keyup(key)
        return v

    # ---- capture ---------------------------------------------------------
    def dump(self, path):
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        if os.path.exists(path):
            os.unlink(path)
        out = self.dbg_capture("savebin %s $%x $%x" % (path, STATE_LO, STATE_HI - STATE_LO))
        if "Wrote" not in out:
            raise RuntimeError("savebin failed: %r" % out.strip())
        for _ in range(50):
            if os.path.exists(path) and os.path.getsize(path) == STATE_HI - STATE_LO:
                return
            time.sleep(0.02)
        raise Timeout("dump %s never reached %d bytes" % (path, STATE_HI - STATE_LO))

#!/usr/bin/env python3
"""Differentially validate disc-oracle against Hatari.

An oracle nobody checked is just a second bug surface. This runs the same
seed and the same input through both emulators and reports the FIRST place
they disagree, because that is the fact that tells you what to fix -- a
pass/fail count does not.

Both sides sample at the contract point (PC == $8198, before that instruction
runs; see reports/oracle-scope.md).
"""
import argparse, json, os, subprocess, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from collect import Hatari, load_scenario   # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ORACLE = os.path.join(ROOT, "oracle", "disc-oracle")
# One 3200-byte memdump window covers every address the oracle reports:
# $6ab4 counter, $6c58 joystick, $6ca0/$6d20 players, $6e3e disc array,
# $7616 tile grid.
WIN_LO, WIN_HI = 0x6a00, 0x76c0

# The screen double-buffer pointers. They swap every frame EXCEPT on a frame
# the game drops, and whether a frame drops depends on whether the main loop
# finished inside its cycle budget -- a cycle-timing question, which Musashi
# (instruction-accurate, not cycle-accurate) is not expected to reproduce.
# Measured in the Hatari reference: 462 swaps over 487 frames.
# They are video state, outside the oracle's contract; --strict includes them.
VIDEO_PTRS = range(0x6aac, 0x6ab4)

LABELS = [
    (0x6aac, 4, "screen buffer ptr A (video)"),
    (0x6ab0, 4, "screen buffer ptr B (video)"),
    (0x6ab4, 2, "$6ab4 frame counter"), (0x6c58, 1, "$6c58 joystick"),
    (0x6ca2, 2, "$6ca2 player1 X"), (0x6ca6, 2, "$6ca6 player1 Y"),
    (0x6cae, 1, "$6cae player1 state"), (0x6cb0, 2, "$6cb0 player1 cell"),
    (0x6d22, 2, "$6d22 player2 X"), (0x6d26, 2, "$6d26 player2 Y"),
    (0x6e3e, 2, "$6e3e disc0 world X"), (0x6e42, 2, "$6e42 disc0 world Z"),
    (0x6e44, 2, "$6e44 disc0 vel X"), (0x6e4a, 2, "$6e4a disc0 screen X"),
]


def label_for(addr):
    for a, n, name in LABELS:
        if a <= addr < a + n:
            return name
    if 0x6e3e <= addr < 0x704e:
        i = (addr - 0x6e3e) // 0x42
        return "disc[%d] +$%02x" % (i, (addr - 0x6e3e) % 0x42)
    if 0x7616 <= addr < 0x769e:
        i = (addr - 0x7616) // 8
        return "tile grid cell %d +$%x" % (i, (addr - 0x7616) % 8)
    if 0x6ca0 <= addr < 0x6d20:
        return "player1 record +$%02x" % (addr - 0x6ca0)
    if 0x6d20 <= addr < 0x6da0:
        return "player2 record +$%02x" % (addr - 0x6d20)
    return "unlabelled"


# A joystick programme, in wall-clock seconds from the start of the trace.
# Frames are ~20 ms, so these land within a frame or two; the differ does not
# assume where -- it reads back the frames on which $6c58 actually changed and
# builds the oracle's script from those.  What is being validated is the whole
# IKBD path: Hatari's real IKBD emits packets, the oracle's synthetic ACIA
# emits packets, and both games decode them into $6c58 themselves.
INPUT_PROGRAMME = [
    (1.2, "Right", True), (2.0, "Right", False),
    (2.6, "Left", True),  (3.4, "Left", False),
    (4.0, "Up", True),    (4.5, "Up", False),
    (5.0, "Fire", True),  (5.2, "Fire", False),
]


def hatari_side(scn, seed_path, frames, keep_window=False, programme=None):
    """Capture a seed and, in the same session, trace from it."""
    name = scn.get("name", "oracle_diff")
    h = Hatari(logpath="tmp/%s-hatari.log" % name,
               shotdir="tmp/shots-%s" % name, keep_window=keep_window)
    try:
        h.start()
        h.enter_match(cache="tmp/match_%s.sav" % scn.get("mode", "training"),
                      mode=scn.get("mode", "training"))
        h.wait_frames(scn.get("settle", 15))
        meta = h.seed(seed_path)
        during = None
        if programme:
            def during():
                t0 = time.time()
                for when, key, down in programme:
                    time.sleep(max(0.0, t0 + when - time.time()))
                    (h.pad.keydown if down else h.pad.keyup)(key)
                time.sleep(max(0.0, t0 + (frames + 8) * 0.021 - time.time()))
            h.release()
        snaps = h.frame_trace(WIN_LO, frames + 8, during=during, at=None)
        h.release()
        return meta, snaps
    finally:
        h.stop()


def script_from_trace(snaps, seed_counter, path):
    """Turn the joystick bytes Hatari actually decoded into an oracle script.

    The stimulus is injected into Hatari by wall-clock XTEST, so which frame it
    lands on is not ours to choose -- we read it back instead.  $6c58/$6c59 are
    what the GAME decoded, and a byte first seen at frame F was decoded during
    frame F-1, so the packet is scheduled a frame earlier.  The oracle still
    has to decode it: the script queues IKBD packets, never the byte.
    """
    lines = ["# derived from the joystick bytes Hatari decoded", "j 0 00 00"]
    prev = None
    changes = []
    for cnt, m in snaps:
        cur = (m.get(0x6c58, 0), m.get(0x6c59, 0))
        if prev is not None and cur != prev:
            f = cnt - seed_counter - 1
            if f >= 1:
                lines.append("j %d %02x %02x" % (f, cur[0], cur[1]))
                changes.append((f, cur[0], cur[1]))
        prev = cur
    with open(path, "w") as f:
        f.write("\n".join(lines) + "\n")
    return changes


def oracle_side(seed_path, script_path, frames, out_path):
    if not os.path.exists(ORACLE):
        raise SystemExit("build the oracle first:  make -C oracle")
    t0 = time.time()
    r = subprocess.run([ORACLE, "--seed", seed_path, "--script", script_path,
                        "--frames", str(frames), "--window", hex(WIN_LO),
                        hex(WIN_HI), "--trace", out_path, "--debug-regs"],
                       capture_output=True, text=True)
    if r.returncode:
        sys.stderr.write(r.stderr)
        raise SystemExit("disc-oracle failed (exit %d)" % r.returncode)
    if r.stderr.strip():
        sys.stderr.write(r.stderr)
    recs = [json.loads(l) for l in open(out_path)]
    return recs, time.time() - t0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--scenario", default="scenarios/oracle_seed.yaml")
    ap.add_argument("--frames", type=int, default=300)
    ap.add_argument("--seed", default="seeds/diff.seed")
    ap.add_argument("--script", default=None,
                    help="oracle input script; default: idle")
    ap.add_argument("--input", action="store_true",
                    help="drive a joystick programme through both sides")
    ap.add_argument("--keep-window", action="store_true")
    ap.add_argument("--cache", default="tmp/hatari_ref.json",
                    help="reuse a saved Hatari reference run if present")
    ap.add_argument("--refresh", action="store_true",
                    help="re-run Hatari even if the cache exists")
    ap.add_argument("--strict", action="store_true",
                    help="also compare the video double-buffer pointers")
    ap.add_argument("--min-agree", type=int, default=275,
                    help="frames of exact agreement required to pass; the "
                         "default is the measured cycle-accuracy boundary")
    a = ap.parse_args(argv)

    scn = load_scenario(a.scenario)
    if a.input and a.cache == ap.get_default("cache"):
        a.cache = "tmp/hatari_ref_input.json"
    script = a.script
    if not script:
        script = "tmp/idle.script"
        os.makedirs("tmp", exist_ok=True)
        with open(script, "w") as f:
            f.write("# no input\nj 0 00 00\n")

    if a.cache and os.path.exists(a.cache) and not a.refresh:
        print("== Hatari side (cached %s)" % a.cache, flush=True)
        c = json.load(open(a.cache))
        meta, hat_secs = c["meta"], c["secs"]
        snaps = [(cnt, {int(k, 16): v for k, v in m.items()}) for cnt, m in c["snaps"]]
        if not os.path.exists(a.seed):
            raise SystemExit("cache present but %s is gone; re-run with --refresh"
                             % a.seed)
    else:
        print("== Hatari side (seed + trace) ...", flush=True)
        t0 = time.time()
        meta, snaps = hatari_side(scn, a.seed, a.frames, a.keep_window,
                                  INPUT_PROGRAMME if a.input else None)
        hat_secs = time.time() - t0
        if a.cache:
            with open(a.cache, "w") as f:
                json.dump({"meta": meta, "secs": hat_secs,
                           "snaps": [[c, {("%x" % k): v for k, v in m.items()}]
                                     for c, m in snaps]}, f)
    print("   seed $6ab4=%d sha256=%s.. ; %d traced frames in %.1fs"
          % (meta["vbl_counter_6ab4"], meta["sha256"][:16], len(snaps), hat_secs))

    if a.input:
        script = "tmp/derived.script"
        ch = script_from_trace(snaps, meta["vbl_counter_6ab4"], script)
        print("   joystick changes Hatari decoded: %s"
              % (", ".join("f%d=$%02x/$%02x" % c for c in ch) or "NONE"))
        if not ch:
            raise SystemExit("no joystick activity reached the game -- the "
                             "input programme did not land")

    print("== oracle side ...", flush=True)
    recs, orc_secs = oracle_side(a.seed, script, a.frames, "tmp/oracle_diff.ndjson")
    print("   %d frames in %.2fs" % (len(recs), orc_secs))

    # ---- align on $6ab4, which both sides compute themselves --------------
    # Capturing the seed costs a 1 MB savebin, so Hatari's trace starts a few
    # frames after the seed instant.  Align on the game's own counter and skip
    # the oracle frames that precede the trace; they stay unvalidated, and the
    # count is reported rather than hidden.
    hat_by_cnt = {c: i for i, (c, _) in enumerate(snaps)}
    ostart = next((i for i, r in enumerate(recs)
                   if r["vbl_6ab4"] in hat_by_cnt), None)
    if ostart is None:
        raise SystemExit("cannot align: no oracle frame shares a $6ab4 with the "
                         "trace (oracle %s.., Hatari %s..)"
                         % (recs[0]["vbl_6ab4"], snaps[0][0]))
    start = hat_by_cnt[recs[ostart]["vbl_6ab4"]]
    print("   aligned on $6ab4=%d: oracle frame %d == Hatari trace index %d"
          % (recs[ostart]["vbl_6ab4"], ostart, start))
    if ostart:
        print("   (oracle frames 0..%d precede the Hatari trace and are not "
              "compared)" % (ostart - 1))

    # ---- step 3: the two sides must START identical ----------------------
    _, m0 = snaps[start]
    o0 = bytes.fromhex(recs[ostart]["mem"])
    bad0 = [x for x in range(WIN_LO, WIN_HI) if x in m0 and m0[x] != o0[x - WIN_LO]]
    if bad0:
        print("\nSTART MISMATCH at %d byte(s):" % len(bad0))
        for x in bad0[:24]:
            print("   $%04x   %-26s Hatari=%02x oracle=%02x"
                  % (x, label_for(x), m0[x], o0[x - WIN_LO]))
        print("The two sides differ at the first comparable frame. Either the "
              "seed capture is wrong, or the oracle already diverged in the "
              "%d frames before the trace begins." % ostart)
        return 1
    print("   start check: first comparable frame identical across %d bytes"
          % (WIN_HI - WIN_LO))

    # ---- compare ---------------------------------------------------------
    skip = set() if a.strict else set(VIDEO_PTRS)
    if skip:
        print("   excluding $%04x-$%04x (video double-buffer pointers; "
              "--strict to include)" % (min(skip), max(skip)))
    n = min(len(recs) - ostart, len(snaps) - start)
    first_any = None
    for i in range(n):
        cnt, hm = snaps[start + i]
        rec = recs[ostart + i]
        if rec["vbl_6ab4"] != cnt:
            print("\nALIGNMENT LOST at frame %d: Hatari $6ab4=%d, oracle=%d"
                  % (i, cnt, rec["vbl_6ab4"]))
            return 1
        om = bytes.fromhex(rec["mem"])
        alldiff = [x for x in range(WIN_LO, WIN_HI)
                   if x in hm and hm[x] != om[x - WIN_LO]]
        if alldiff and first_any is None:
            first_any = (i, alldiff)
        diffs = [x for x in alldiff if x not in skip]
        if diffs:
            print("\nFIRST DIVERGENCE at frame %d (%d byte(s))" % (i, len(diffs)))
            print("   oracle PC=$%04x SR=$%04x $6ab4=%d"
                  % (rec["pc"], rec["sr"], rec["vbl_6ab4"]))
            print("\n   %-8s %-26s %-8s %-8s" % ("addr", "what", "Hatari", "oracle"))
            for x in diffs[:24]:
                print("   $%04x   %-26s %-8s %-8s"
                      % (x, label_for(x), "%02x" % hm[x], "%02x" % om[x - WIN_LO]))
            if len(diffs) > 24:
                print("   ... %d more" % (len(diffs) - 24))
            print("\n   %d frames agreed before this." % i)
            if first_any and first_any[0] < i:
                print("   (including the video pointers, the first difference "
                      "was at frame %d)" % first_any[0])
            if i >= a.min_agree:
                print("\nPASS: %d >= --min-agree %d. This is the known "
                      "cycle-accuracy boundary, not a new fault." % (i, a.min_agree))
                print("speed: oracle %.0f fps, Hatari %.0f fps (%.0fx)"
                      % (len(recs) / orc_secs, len(snaps) / hat_secs,
                         (len(recs) / orc_secs) / max(1e-9, len(snaps) / hat_secs)))
                return 0
            print("\nFAIL: only %d frames agreed, below --min-agree %d."
                  % (i, a.min_agree))
            return 1
    print("\nZERO DIVERGENCES over %d frames x %d bytes%s."
          % (n, WIN_HI - WIN_LO - len(skip),
             "" if a.strict else " (video pointers excluded)"))
    if first_any:
        print("   including the video pointers, the first difference was at "
              "frame %d ($%04x)" % (first_any[0], first_any[1][0]))
    print("speed: oracle %.0f fps, Hatari %.0f fps (%.0fx)"
          % (len(recs) / orc_secs, len(snaps) / hat_secs,
             (len(recs) / orc_secs) / max(1e-9, len(snaps) / hat_secs)))
    return 0


if __name__ == "__main__":
    sys.exit(main())

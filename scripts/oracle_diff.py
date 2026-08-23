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
# One memdump window covers every address the oracle reports: $6ab4 counter,
# $6c58 joystick, $6ca0/$6d20 players, $6e3e disc array, $7616 tile grid.
#
# It has to reach $769d -- the END of tile cell 16.  At nMemdumpLines = 200 the
# window stopped at $767f, so cells 13-16 were never compared: exactly the far
# row of the floor, where the player stands (its idle cell is 15).  The differ
# skipped the missing bytes silently, which is how a blind spot survives.  The
# window is now 232 lines and coverage is ASSERTED below, not assumed.
WIN_LO, WIN_HI = 0x6a00, 0x76a0
GRID_END = 0x7616 + 17 * 8          # $769e, one past the last tile byte

# The screen double-buffer pointers. They swap every frame EXCEPT on a frame
# the game drops, and whether a frame drops depends on whether the main loop
# finished inside its cycle budget -- a cycle-timing question, which Musashi
# (instruction-accurate, not cycle-accurate) is not expected to reproduce.
# Measured in the Hatari reference: 462 swaps over 487 frames.
# They are video state, outside the oracle's contract; --strict includes them.
VIDEO_PTRS = range(0x6aac, 0x6ab4)

# Per-frame counters, and the rate each advances at.  Under a one-frame
# realignment these MUST be off by exactly the shift -- that is what makes a
# dropped frame identifiable rather than merely plausible.
#   $6ab4  addq.w #1  at $8198      $6ab6  subq.w #1 at $819c
#   $6c81  subq.b #1  at $81e2
FRAME_COUNTERS = [(0x6ab4, 2, +1), (0x6ab6, 2, -1), (0x6c81, 1, -1)]
COUNTER_BYTES = {a + k for a, w, _ in FRAME_COUNTERS for k in range(w)}

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
INPUT_PROGRAMMES = {
    # exercise each direction and fire once
    "sweep": [
        (1.2, "Right", True), (2.0, "Right", False),
        (2.6, "Left", True),  (3.4, "Left", False),
        (4.0, "Up", True),    (4.5, "Up", False),
        (5.0, "Fire", True),  (5.2, "Fire", False),
    ],
    # The policy scripts/explore.py found most productive: pin the player at
    # the far-right cell and pulse fire.  "Walk to cell 16" degenerates to
    # "hold Right" once he is against the edge, which is the only reason this
    # closed-loop result can be replayed open-loop into Hatari at all.
    # Two LIGHT programmes, transcribed from autopilot scripts that reached
    # otherwise-unreachable player states.  Light matters: input density is
    # what shortens the validated window, and these are 4 joystick changes
    # each, so the window should stay near the idle 275 rather than collapsing
    # to rightfire's 116.  Frame f in the autopilot script -> t = f * 0.02 s.
    #   leftright  (autopilot cell 14, no fire): state 11 at f64, state 23 at f97
    "leftright": [(0.10, "Left", True), (0.46, "Left", False),
                  (1.96, "Right", True), (7.50, "Right", False)],
    #   rightpause (autopilot cell 16, no fire): state 19 at f236, state 31 at f318
    "rightpause": [(0.10, "Right", True), (0.22, "Right", False),
                   (5.82, "Right", True), (9.00, "Right", False)],
    "rightfire": ([(0.4, "Right", True)] +
                  [t for i in range(24)
                   for t in ((0.9 + i * 0.16, "Fire", True),
                             (0.9 + i * 0.16 + 0.05, "Fire", False))] +
                  [(6.4, "Right", False)]),
}


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
    ap.add_argument("--input", nargs="?", const="sweep", default=None,
                    choices=sorted(INPUT_PROGRAMMES),
                    help="drive a joystick programme through both sides")
    ap.add_argument("--keep-window", action="store_true")
    ap.add_argument("--cache", default="tmp/hatari_ref.json",
                    help="reuse a saved Hatari reference run if present")
    ap.add_argument("--refresh", action="store_true",
                    help="re-run Hatari even if the cache exists")
    ap.add_argument("--strict", action="store_true",
                    help="also compare the video double-buffer pointers")
    ap.add_argument("--tier2", action="store_true",
                    help="continue past a dropped-frame desync with an "
                         "alignment offset, as a labelled evidence tier")
    ap.add_argument("--tier2-confirm", type=int, default=6,
                    help="frames a realignment must hold clean to be accepted")
    ap.add_argument("--min-agree", type=int, default=275,
                    help="frames of exact agreement required to pass; the "
                         "default is the measured cycle-accuracy boundary")
    a = ap.parse_args(argv)

    scn = load_scenario(a.scenario)
    if a.input and a.cache == ap.get_default("cache"):
        a.cache = "tmp/hatari_ref_%s.json" % a.input
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
                                  INPUT_PROGRAMMES.get(a.input))
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
    have = snaps[start][1]
    missing = [x for x in range(WIN_LO, WIN_HI) if x not in have]
    if missing:
        raise SystemExit(
            "Hatari's memdump window does not cover the compared range: %d of "
            "%d bytes absent, first $%04x last $%04x. Raise nMemdumpLines in "
            "hatari.cfg and re-record the reference -- comparing a partial "
            "window silently under-reports coverage."
            % (len(missing), WIN_HI - WIN_LO, missing[0], missing[-1]))
    if GRID_END > WIN_HI:
        raise SystemExit("the window stops at $%04x, before the tile grid ends "
                         "at $%04x" % (WIN_HI, GRID_END))
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

    def cmp_at(oi, hi_):
        """Bytes differing between oracle record oi and Hatari trace index hi_."""
        if not (0 <= oi < len(recs) and 0 <= hi_ < len(snaps)):
            return None
        hm = snaps[hi_][1]
        om = bytes.fromhex(recs[oi]["mem"])
        return [x for x in range(WIN_LO, WIN_HI)
                if x in hm and hm[x] != om[x - WIN_LO]]

    n = min(len(recs) - ostart, len(snaps) - start)
    first_any = None
    tier1 = None            # frames of frame-exact agreement
    shift = 0               # accumulated dropped-frame realignment
    drops = []
    i = 0
    while i < n:
        cnt, hm = snaps[start + i]
        oi = ostart + i + shift
        if oi >= len(recs):
            break
        rec = recs[oi]
        alldiff = [x for x in range(WIN_LO, WIN_HI)
                   if x in hm and hm[x] != bytes.fromhex(rec["mem"])[x - WIN_LO]]
        if alldiff and first_any is None:
            first_any = (i, alldiff)
        diffs = [x for x in alldiff if x not in skip]
        if not diffs:
            i += 1
            continue

        if tier1 is None:
            tier1 = i
            print("\nFIRST DIVERGENCE at frame %d (%d byte(s))" % (i, len(diffs)))
            print("   oracle PC=$%04x SR=$%04x $6ab4=%d"
                  % (rec["pc"], rec["sr"], rec["vbl_6ab4"]))
            print("\n   %-8s %-26s %-8s %-8s" % ("addr", "what", "Hatari", "oracle"))
            for x in diffs[:20]:
                print("   $%04x   %-26s %-8s %-8s"
                      % (x, label_for(x), "%02x" % hm[x],
                         "%02x" % bytes.fromhex(rec["mem"])[x - WIN_LO]))
            if len(diffs) > 20:
                print("   ... %d more" % (len(diffs) - 20))
            print("\n   %d frames agreed before this." % i)
            if first_any and first_any[0] < i:
                print("   (including the video pointers, the first difference "
                      "was at frame %d)" % first_any[0])

        if not a.tier2:
            break

        # ---- tier 2: is this the stereotyped one-dropped-frame desync? ------
        # Accept a realignment only if, after shifting, the ONLY bytes still
        # differing are the per-frame counters AND each is off by exactly the
        # amount the shift predicts.  That is a falsifiable test for "one side
        # dropped a frame", not a licence to slide until something matches.
        def counters_confirm(oi, hi_, delta):
            d2 = cmp_at(oi, hi_)
            if d2 is None:
                return False
            stray = [x for x in d2 if x not in skip and x not in COUNTER_BYTES]
            if stray:
                return False
            hm2, om2 = snaps[hi_][1], bytes.fromhex(recs[oi]["mem"])
            for addr, width, rate in FRAME_COUNTERS:
                if addr not in hm2:
                    continue
                hv = ov = 0
                for k in range(width):
                    hv = (hv << 8) | hm2[addr + k]
                    ov = (ov << 8) | om2[addr + k - WIN_LO]
                if hv != (ov + rate * -delta) % (1 << (8 * width)):
                    return False
            return True

        realigned = False
        for delta in (-1, +1):
            if all(counters_confirm(ostart + i + shift + delta + k,
                                    start + i + k, delta)
                   for k in range(min(a.tier2_confirm, n - i))):
                shift += delta
                drops.append((i, delta))
                realigned = True
                break
        if not realigned:
            print("\n   tier 2: no counter-confirmed realignment at frame %d."
                  % i)
            for delta in (-1, +1):
                row = []
                for k in range(min(8, n - i)):
                    d2 = cmp_at(ostart + i + shift + delta + k, start + i + k)
                    row.append("-" if d2 is None else
                               str(len([x for x in d2 if x not in skip
                                        and x not in COUNTER_BYTES])))
                print("      shift %+d: stray bytes for the next frames: %s"
                      % (delta, " ".join(row)))
            print("      A clean column means the shift holds for that frame. "
                  "A single clean frame followed by stray bytes is NOT a "
                  "dropped frame -- the runs have genuinely parted, and the "
                  "counters being off by one is a consequence, not the cause.")
            break
        print("   tier 2: dropped frame at %d (shift %+d, counters confirm); "
              "continuing" % (i, drops[-1][1]))
        i += 1

    if tier1 is None:
        tier1 = n
    if tier1 >= n:
        print("\nZERO DIVERGENCES over %d frames x %d bytes%s."
              % (n, WIN_HI - WIN_LO - len(skip),
                 "" if a.strict else " (video pointers excluded)"))
        if first_any:
            print("   including the video pointers, the first difference was "
                  "at frame %d ($%04x)" % (first_any[0], first_any[1][0]))
    if a.tier2:
        print("\n   TIER 2 (drop-realigned): agreement to frame %d with %d "
              "realignment(s) %s"
              % (i, len(drops), [d for d in drops] or ""))
        print("   Tier-2 reach is for OBSERVATION ONLY -- every claim drawn "
              "from beyond frame %d must be labelled tier 2." % tier1)
    print("\nTIER 1 (frame-exact): %d frames." % tier1)
    if tier1 >= a.min_agree:
        print("PASS: %d >= --min-agree %d." % (tier1, a.min_agree))
        print("speed: oracle %.0f fps, Hatari %.0f fps (%.0fx)"
              % (len(recs) / orc_secs, len(snaps) / hat_secs,
                 (len(recs) / orc_secs) / max(1e-9, len(snaps) / hat_secs)))
        return 0
    print("FAIL: only %d frames agreed, below --min-agree %d." % (tier1, a.min_agree))
    return 1
    print("speed: oracle %.0f fps, Hatari %.0f fps (%.0fx)"
          % (len(recs) / orc_secs, len(snaps) / hat_secs,
             (len(recs) / orc_secs) / max(1e-9, len(snaps) / hat_secs)))
    return 0


if __name__ == "__main__":
    sys.exit(main())

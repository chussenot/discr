#!/usr/bin/env python3
"""Mint a new frame-boundary seed part-way along a validated programme.

The differential window is short and gets shorter as input gets busier, so
reaching a late event by running longer does not work. Start closer instead:
replay a programme that is already validated to frame N, stop at some frame
F <= N, and capture a seed there. Because the prefix up to F is byte-for-byte
Hatari-verified, the new seed's provenance is frame-exact rather than merely
plausible.

Seeds are gitignored (they are derived from the disk image); their provenance
goes in seeds/MANIFEST.md, which is committed.
"""
import argparse, hashlib, json, os, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from collect import Hatari                                   # noqa: E402
from oracle_diff import INPUT_PROGRAMMES, WIN_LO             # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MANIFEST = os.path.join(ROOT, "seeds", "MANIFEST.md")


def relay(parent, programme, at_frame, out, mode="challenge", settle=15,
          trace_frames=0, cache=None):
    """Replay `programme` from the parent seed's situation, seed at `at_frame`.

    `at_frame` is counted in the parent seed's own $6ab4 units, so it names the
    same instant the differ reports.
    """
    pmeta = json.load(open(parent + ".json"))
    target = pmeta["vbl_counter_6ab4"] + at_frame
    h = Hatari(logpath="tmp/relay.log", shotdir="tmp/shots-relay")
    try:
        h.start()
        h.enter_match(cache="tmp/match_%s.sav" % mode, mode=mode)
        h.wait_frames(settle)
        here = h.peek_word(0x6ab4)
        if here is None or here > target:
            raise SystemExit("already past frame %d (at %s); the parent seed's "
                             "situation is not reproducible from this savestate"
                             % (target, here))
        # Drive the programme on wall-clock, exactly as the differ does, but
        # poll the counter WHILE waiting: a programme with a long gap between
        # presses would otherwise sleep straight past the mint frame.
        t0 = time.time()
        for when, key, down in programme:
            while time.time() < t0 + when:
                if h.peek_word(0x6ab4) >= target:
                    break
                time.sleep(min(0.05, max(0.0, t0 + when - time.time())))
            if h.peek_word(0x6ab4) >= target:
                break
            (h.pad.keydown if down else h.pad.keyup)(key)
        h.release()
        # Let the release packets be DECODED before freezing the machine.
        # Seeding mid-press bakes $6c58 = $80 into the image; the Hatari
        # reference then decodes the release a few frames later while an idle
        # oracle never does, and verification fails on the joystick byte for a
        # reason that has nothing to do with emulation.
        for _ in range(40):
            if h.peek_word(0x6c58) == 0:
                break
            h.wait_frames(2)
        else:
            raise SystemExit("joystick byte never returned to 0 after release")
        h.run_to_counter(target)
        meta = h.seed(out)
        if meta.get("joystick_6c58"):
            raise SystemExit("seed captured with input held ($6c58=$%02x)"
                             % meta["joystick_6c58"])
        if trace_frames:
            # Capture the Hatari reference from the SAME session, immediately
            # after minting: the seed and the trace then share a situation by
            # construction, which is what the verification compares against.
            t0 = time.time()
            snaps = h.frame_trace(WIN_LO, trace_frames + 8)
            with open(cache, "w") as f:
                json.dump({"meta": meta, "secs": time.time() - t0,
                           "snaps": [[c, {("%x" % k): v for k, v in m.items()}]
                                     for c, m in snaps]}, f)
            print("   Hatari reference: %d frames -> %s" % (len(snaps), cache))
        # NOT "sha256": a nested key of that name shadows the seed's own for
        # any consumer that greps for the first match.
        meta["parent"] = {"seed": os.path.basename(parent),
                          "parent_sha256": pmeta["sha256"],
                          "counter": pmeta["vbl_counter_6ab4"]}
        meta["relay"] = {"at_frame": at_frame, "target_counter": target}
        json.dump(meta, open(out + ".json", "w"), indent=1, sort_keys=True)
        return meta
    finally:
        h.stop()


def record(meta, out, programme_name, validated_to):
    os.makedirs(os.path.dirname(MANIFEST), exist_ok=True)
    new = not os.path.exists(MANIFEST)
    with open(MANIFEST, "a") as f:
        if new:
            f.write("# Seed provenance\n\n"
                    "Seed binaries are derived from the disk image and are "
                    "gitignored; this file records where each one came from so "
                    "a result can be traced back to a frame-exact origin.\n\n"
                    "A relayed seed is only listed once `scripts/seed_verify.sh` "
                    "has confirmed it: both emulators identical at frame 0 and "
                    "agreeing for a short differential run.\n\n"
                    "| seed | sha256 | $6ab4 | parent | programme | frame | "
                    "parent validated to |\n"
                    "|---|---|---|---|---|---|---|\n")
        p = meta.get("parent", {})
        r = meta.get("relay", {})
        f.write("| `%s` | `%s` | %d | `%s` | %s | %s | %s |\n"
                % (os.path.basename(out), meta["sha256"][:16],
                   meta["vbl_counter_6ab4"], p.get("seed", "-"),
                   programme_name, r.get("at_frame", "-"), validated_to))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--parent", default="seeds/diff.seed")
    ap.add_argument("--programme", default="rightfire",
                    choices=sorted(INPUT_PROGRAMMES))
    ap.add_argument("--at-frame", type=int, required=True,
                    help="frame along the programme, in the parent's $6ab4 units")
    ap.add_argument("--validated-to", default="?",
                    help="how far the differ validated that programme")
    ap.add_argument("--out", required=True)
    ap.add_argument("--trace-frames", type=int, default=140,
                    help="also capture a Hatari reference for verification")
    a = ap.parse_args(argv)
    cache = "tmp/hatari_ref_%s.json" % os.path.basename(a.out).replace(".seed", "")
    meta = relay(a.parent, INPUT_PROGRAMMES[a.programme], a.at_frame, a.out,
                 trace_frames=a.trace_frames, cache=cache)
    print("minted %s  $6ab4=%d  sha256=%s.."
          % (a.out, meta["vbl_counter_6ab4"], meta["sha256"][:16]))
    record(meta, a.out, a.programme, a.validated_to)
    print("provenance appended to", MANIFEST)
    print("verify with:  ./scripts/seed_verify.sh %s %s" % (a.out, cache))
    return 0


if __name__ == "__main__":
    sys.exit(main())

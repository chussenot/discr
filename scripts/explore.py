#!/usr/bin/env python3
"""Search the input space with disc-oracle's closed-loop autopilot.

Blind fuzzing is the wrong tool: catching a disc means being somewhere
specific when it arrives. The oracle can read the game's own state each frame
and steer, so "walk to cell N and fire" is a servo, not a search. What is
searched is only the handful of policy parameters.

Everything found is emitted as a plain script so it can be replayed through
scripts/oracle_diff.py and validated against Hatari -- otherwise this would be
reverse-engineering the oracle rather than the game.
"""
import argparse, json, os, subprocess, sys
from collections import Counter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ORACLE = os.path.join(ROOT, "oracle", "disc-oracle")
# Scores / counters the disc engine touches at $a630-$a63c.
COUNTERS = {0x6d0a: "ctr_6d0a", 0x6d0c: "ctr_6d0c",
            0x6d8a: "ctr_6d8a", 0x6d8c: "ctr_6d8c"}


def run(seed, frames, cell, period, start, tag, outdir="tmp/explore"):
    os.makedirs(outdir, exist_ok=True)
    scr = "%s/%s.script" % (outdir, tag)
    tr = "%s/%s.ndjson" % (outdir, tag)
    cmd = [ORACLE, "--seed", seed, "--frames", str(frames),
           "--autopilot", str(cell), str(period), str(start),
           "--emit-script", scr, "--window", "0x6a00", "0x76c0", "--trace", tr]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode:
        return None, r.stderr.strip().splitlines()[:1]
    return [json.loads(l) for l in open(tr)], scr


def summarise(recs):
    s = {}
    s["p1_states"] = sorted({r["player"][0]["state"] for r in recs})
    s["p2_states"] = sorted({r["player"][1]["state"] for r in recs})
    owners = Counter()
    live = 0
    for r in recs:
        n = 0
        for d in r["disc"]:
            f = d["flag"]
            f = f - 65536 if f > 32767 else f
            if f:
                owners[f] += 1
                n += 1
        live = max(live, n)
    s["disc_owner_values"] = sorted(owners)
    s["max_live_discs"] = live
    mem = [bytes.fromhex(r["mem"]) for r in recs]
    lo = recs[0]["win_lo"]
    s["counters"] = {}
    for a, name in COUNTERS.items():
        vals = [(m[a - lo] << 8) | m[a - lo + 1] for m in mem]
        if len(set(vals)) > 1:
            s["counters"][name] = (vals[0], vals[-1])
    s["cells"] = sorted({r["player"][0]["cell"] for r in recs})
    return s


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--seed", default="seeds/diff.seed")
    ap.add_argument("--frames", type=int, default=340,
                    help="keep inside the differentially validated window")
    a = ap.parse_args(argv)
    if not os.path.exists(ORACLE):
        raise SystemExit("build the oracle first: mise run oracle")

    baseline, _ = run(a.seed, a.frames, 0, 0, 0, "idle")
    base = summarise(baseline)
    print("baseline (no input): p1 states %s, max live discs %d, counters %s"
          % (base["p1_states"], base["max_live_discs"], base["counters"]))
    known = set(base["p1_states"])

    rows = []
    for cell in range(9, 17):
        for period, start in ((0, 0), (8, 20), (16, 20), (40, 60)):
            tag = "c%d_p%d_s%d" % (cell, period, start)
            recs, scr = run(a.seed, a.frames, cell, period, start, tag)
            if recs is None:
                print("  %-14s ORACLE ABORT: %s" % (tag, scr))
                continue
            s = summarise(recs)
            new = sorted(set(s["p1_states"]) - known)
            rows.append((len(new), len(s["counters"]), s["max_live_discs"],
                         tag, s, new, scr))
    rows.sort(reverse=True)
    print("\n%-14s %-30s %-6s %-5s %s"
          % ("policy", "p1 states", "discs", "new", "counters that moved"))
    for _, _, live, tag, s, new, scr in rows:
        print("%-14s %-30s %-6d %-5s %s"
              % (tag, s["p1_states"], live,
                 ",".join(map(str, new)) or "-",
                 ",".join(s["counters"]) or "-"))
    interesting = [r for r in rows if r[0] or r[1]]
    print("\n%d policy/policies produced something new." % len(interesting))
    for _, _, _, tag, s, new, scr in interesting[:5]:
        print("   %s -> new states %s, counters %s, script %s"
              % (tag, new, s["counters"], scr))
    return 0


if __name__ == "__main__":
    sys.exit(main())

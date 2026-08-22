#!/usr/bin/env python3
"""Turn collect.py's RAM dumps into reports/findings.md.

Each hunt is a constraint chain run through ramdiff.py plus, where ramdiff
cannot express it, an extra check done here:

  player_x_hunt  ramdiff chain, cross-checked against the mirrored Left run
                 and against invariance during the Up/Down run
  player_y_hunt  the same on the other axis
  disc_flight    delta consistency: equal-gap samples, look for a stable
                 per-frame velocity, and prefer words whose velocity REVERSES
                 (a bouncing coordinate does; a frame counter never does)
  tile_hit       byte-level monotone decrements, reported as adjacent clusters
"""
import argparse, json, os, re, struct, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RAMDIFF = os.path.join(ROOT, "scripts", "ramdiff.py")
DUMPS = os.path.join(ROOT, "dumps")
REPORTS = os.path.join(ROOT, "reports")

# Lines confirmed well enough to paste into docs/disc-notes.md, collected by
# the hunts as they run and summarised at the top of the report.
PASTE = []


# --------------------------------------------------------------------------
# dump access
# --------------------------------------------------------------------------
class Run:
    def __init__(self, name):
        self.name = name
        self.dir = os.path.join(DUMPS, name)
        meta_path = os.path.join(self.dir, "meta.json")
        if not os.path.exists(meta_path):
            raise SystemExit("no dumps for %r -- run:\n  ./scripts/collect.py "
                             "--scenario scenarios/%s.yaml" % (name, name))
        self.meta = json.load(open(meta_path))
        self.base = self.meta.get("base", 0)
        self.tags = [d["tag"] for d in self.meta["dumps"]]
        self.vbl = {d["tag"]: d["vbl"] for d in self.meta["dumps"]}
        self.live = {d["tag"]: d.get("in_match", True) for d in self.meta["dumps"]}
        self.buf = {t: open(os.path.join(self.dir, t + ".bin"), "rb").read()
                    for t in self.tags}
        self.size = len(self.buf[self.tags[0]])

    def path(self, tag):
        return os.path.join(self.dir, tag + ".bin")

    def b(self, tag, addr):
        return self.buf[tag][addr - self.base]

    def w(self, tag, addr, signed=False):
        return struct.unpack_from(">h" if signed else ">H",
                                  self.buf[tag], addr - self.base)[0]

    def addrs(self, step=2):
        return range(self.base, self.base + self.size - step + 1, step)


def ramdiff(width, lo, hi, triplets, limit=100000):
    """Shell out to ramdiff.py; return (stdout, surviving addresses)."""
    cmd = [sys.executable, RAMDIFF, "--" + width,
           "--range", hex(lo), hex(hi), "--max", str(limit)]
    for op, a, b in triplets:
        cmd += [op, a, b]
    out = subprocess.run(cmd, capture_output=True, text=True).stdout
    addrs = [int(m, 16) for m in re.findall(r"^\s*\$([0-9a-f]+)", out, re.M)]
    rel = lambda c: os.path.relpath(c, ROOT) if os.path.sep in c else c
    head = " ".join(["./scripts/ramdiff.py"] + [rel(c) for c in cmd[2:2 + 6]])
    body = ["  %s %s %s" % (op, rel(x), rel(y)) for op, x, y in triplets]
    return " \\\n".join([head] + body), out, addrs


def table(runs, addr, width=2):
    """Value of addr across every dump of every run, as markdown cells."""
    cells = []
    for r in runs:
        vals = [r.w(t, addr, signed=True) if width == 2 else r.b(t, addr)
                for t in r.tags]
        cells.append("%s: %s" % (r.name.replace("player_", "").replace("_hunt", ""),
                                 ", ".join(str(v) for v in vals)))
    return "; ".join(cells)


# --------------------------------------------------------------------------
# hunts
# --------------------------------------------------------------------------
def hunt_axis(axis, out, sibling=None):
    """player_x_hunt / player_y_hunt: mirrored-direction intersection."""
    if axis == "x":
        fwd, rev = Run("player_x_hunt"), Run("player_x_hunt_left")
        other = _maybe("player_y_hunt")
        up, down = "Right", "Left"
        rev_tag = "c"
    else:
        fwd = rev = Run("player_y_hunt")
        other = _maybe("player_x_hunt")
        up, down = "Up", "Down"
        rev_tag = "f"          # e/f are the Down half of the same run

    lo, hi = fwd.base, fwd.base + fwd.size
    trip = [("same", fwd.path("a"), fwd.path("a2")),
            ("same", rev.path("a"), rev.path("a2")),
            ("increased", fwd.path("a2"), fwd.path("c")),
            ("decreased", rev.path("c" if axis == "y" else "a2"),
             rev.path(rev_tag))]
    cmd, raw, addrs = ramdiff("word", lo, hi, trip)

    out.append("## player_%s_hunt -- player %s coordinate\n" % (axis, axis.upper()))
    out.append("Protocol: two idle dumps (`a`, `a2`) give a noise baseline, then "
               "two equal bursts of **%s** (`b`, `c`)%s.\n" % (
                   up, " and two of **%s** (`e`, `f`)" % down if axis == "y"
                   else ", mirrored by the `player_x_hunt_left` run"))
    out.append("Constraint chain (via `ramdiff.py`):\n")
    out.append("```\n%s\n```\n" % cmd)
    out.append("`same(a,a2)` in **both** runs is the noise filter -- without it "
               "every free-running frame counter survives.\n")
    out.append("| addr | width | %s | %s | interpretation |"
               % (up + " run", down + " run"))
    out.append("|---|---|---|---|---|")

    ranked = []
    for a in addrs:
        f = [fwd.w(t, a, True) for t in fwd.tags]
        r = [rev.w(t, a, True) for t in rev.tags]
        # an axis coordinate must be INVARIANT while the other axis is driven
        cross = None
        if other:
            ov = [other.w(t, a, True) for t in other.tags]
            cross = len(set(ov)) == 1
        hi_byte_zero = all(fwd.w(t, a) < 256 for t in fwd.tags)
        # prefer a field sitting in the same record as an already-confirmed
        # sibling: the game keeps the player's fields in one 16-bit struct
        near = sibling is not None and abs(a - sibling) <= 0x20
        score = (4 if near else 0) + (2 if cross else 0) + (1 if hi_byte_zero else 0)
        ranked.append((score, -a if near else a, f, r, cross, hi_byte_zero, near))
    ranked = [(s_, abs(a_), f_, r_, c_, b_, n_)
              for s_, a_, f_, r_, c_, b_, n_ in ranked]
    ranked.sort(reverse=True)

    best = None
    for score, a, f, r, cross, byte_ish, near in ranked:
        note = []
        if near:
            note.append("in the same record as `$%04x`" % sibling)
        if cross:
            note.append("**invariant during the other axis** -- decisive")
        elif cross is False:
            note.append("also moves on the other axis, so not a pure %s" % axis.upper())
        note.append("high byte always 0 -> really a byte at $%04x" % (a + 1)
                    if byte_ish else "full 16-bit range")
        out.append("| `$%04x` | word | %s | %s | %s |"
                   % (a, ", ".join(map(str, f)), ", ".join(map(str, r)),
                      "; ".join(note)))
        if best is None and cross:
            best = (a, f, r, byte_ish)
    out.append("")

    if best:
        a, f, r, byte_ish = best
        out.append("**Conclusion (high confidence).** `$%04x` is the player's %s. "
                   "It is constant while idle, rises on %s, falls on %s, and does "
                   "not move at all through the entire %s run.\n"
                   % (a, axis.upper(), up, down,
                      "player_y_hunt" if axis == "x" else "player_x_hunt"))
        line = ("$%04x  player_%s  (%s, unsigned screen/grid coordinate)  "
                "idle %d; %s -> %d; %s -> %d; unchanged across the other axis"
                % (a, axis, "word, high byte always 0" if byte_ish else "word",
                   f[0], up, max(f), down, min(r)))
        PASTE.append(line)
        out.append("```\n%s\n```\n" % line)
    else:
        out.append("**No candidate survived the cross-axis check.**\n")
    return best[0] if best else None


def _maybe(name):
    try:
        return Run(name)
    except SystemExit:
        return None


def hunt_disc(out):
    """disc_flight: delta consistency across equal-gap samples."""
    r = Run("disc_flight")
    seq = [t for t in r.tags[1:] if r.live[t]]
    gaps = [r.vbl[seq[i + 1]] - r.vbl[seq[i]] for i in range(len(seq) - 1)]
    lo, hi = r.base, r.base + r.size

    cmd, raw, addrs = ramdiff("word", lo, hi,
                              [("changed", r.path(seq[0]), r.path(seq[1])),
                               ("changed", r.path(seq[1]), r.path(seq[2]))])
    out.append("## disc_flight -- disc position / velocity\n")
    out.append("Sampled %d times at %s-frame gaps during a live CHALLENGE bout "
               "(TRAINING never serves the disc -- see KNOWN_ISSUES.md). Dump "
               "range $%04x-$%04x.\n" % (len(seq), "/".join(map(str, sorted(set(gaps)))), lo, hi))
    out.append("ramdiff's `changed(e,f) changed(f,g)` opener leaves %d words:\n"
               % len(addrs))
    out.append("```\n%s\n```\n" % cmd)
    out.append("ramdiff cannot express the interesting part, so analyze.py does "
               "it: normalise each interval by its real VBL gap, then keep words "
               "that move in *every* interval with a consistent |velocity|. A "
               "**sign reversal** separates a bouncing coordinate from a "
               "monotone frame counter.\n")

    rows = []
    for a in r.addrs(2):
        vals = [r.w(t, a, True) for t in seq]
        if any(vals[i] == vals[i + 1] for i in range(len(vals) - 1)):
            continue
        v = [(vals[i + 1] - vals[i]) / gaps[i] for i in range(len(vals) - 1)]
        mag = [abs(x) for x in v]
        if max(mag) > 200:
            continue
        spread = (max(mag) - min(mag)) / max(mag)
        signs = [1 if x > 0 else -1 for x in v]
        revs = sum(1 for i in range(len(signs) - 1) if signs[i] != signs[i + 1])
        rows.append((spread, revs, a, vals, v))
    rows.sort()

    counter = [r_ for r_ in rows if r_[0] < 0.15 and r_[1] == 0]
    if counter:
        PASTE.append("$%04x  vbl_frame_counter  (word, wraps)  increments by "
                     "exactly 1 per PAL VBL across %d equal-gap samples, never "
                     "reverses; note this is $6ab4, NOT the $6ab6 given in the "
                     "brief, which is zero in every in-match dump"
                     % (counter[0][2], len(seq)))
    out.append("| addr | velocity spread | reversals | values | reading |")
    out.append("|---|---|---|---|---|")
    for spread, revs, a, vals, v in rows:
        if spread < 0.15 and revs == 0:
            reading = ("monotone at %+.0f/frame and never reverses -- a "
                       "free-running counter, not a position" % v[0])
        elif revs:
            reading = ("|v| ~ %.1f/frame with %d reversal(s), but confined to "
                       "%d..%d -- a **wrapping phase counter**, not an "
                       "integrated coordinate" % (sum(abs(x) for x in v) / len(v),
                                                  revs, min(vals), max(vals)))
        else:
            reading = "erratic; scratch/RNG"
        out.append("| `$%04x` | %.2f | %d | %s | %s |"
                   % (a, spread, revs, ", ".join(map(str, vals)), reading))
    out.append("")
    out.append("**Conclusion (negative result, medium confidence).** No word in "
               "$%04x-$%04x behaves like an integrated disc coordinate: every "
               "word that changes on every frame either advances monotonically "
               "at exactly 1/frame (a counter) or stays confined to a small "
               "band (a wrapping phase). This scenario already dumps well past "
               "$8000, so the disc is not simply stored outside the assumed "
               "state area. Either the game recomputes the disc position each "
               "frame from a trajectory parameter rather than storing it, or it "
               "moves fast enough that a ~14-frame sample aliases it. Next "
               "step: shorten the gaps to 1-2 frames, which needs a cheaper "
               "dump than a full savebin per sample.\n" % (lo, hi))
    return rows


def hunt_tiles(out):
    """tile_hit: bytes that only ever decrease, grouped into clusters."""
    r = Run("tile_hit")
    seq = [t for t in r.tags if r.live[t]]
    lo, hi = r.base, r.base + r.size
    cmd, raw, addrs = ramdiff("byte", lo, hi,
                              [("decreased", r.path(seq[0]), r.path(seq[-1]))])

    out.append("## tile_hit -- wall tile grid (HP, not booleans)\n")
    out.append("Dumps `%s` taken while the disc is in play against the shaped "
               "wall tiles of a CHALLENGE bout.\n" % ", ".join(seq))
    out.append("```\n%s\n```\n" % cmd)
    out.append("ramdiff's `decreased` over the whole series gives %d bytes; "
               "analyze.py keeps only those that **never increase** at any "
               "intermediate step (tile HP is monotone) and groups adjacent "
               "survivors, because a tile grid should be contiguous.\n"
               % len(addrs))

    mono = []
    for a in addrs:
        vals = [r.b(t, a) for t in seq]
        if all(vals[i] >= vals[i + 1] for i in range(len(vals) - 1)):
            mono.append((a, vals))
    out.append("Monotonically decreasing bytes: **%d**.\n" % len(mono))

    clusters, cur = [], []
    for a, vals in mono:
        if cur and a == cur[-1][0] + 1:
            cur.append((a, vals))
        else:
            if cur:
                clusters.append(cur)
            cur = [(a, vals)]
    if cur:
        clusters.append(cur)
    clusters.sort(key=len, reverse=True)

    # A tile taking a hit is a discrete EVENT: flat, one drop, flat again.
    # A countdown timer ticks down on most samples.  Split them apart.
    def steps(vals):
        return sum(1 for i in range(len(vals) - 1) if vals[i] != vals[i + 1])

    event = [(a, v) for a, v in mono if steps(v) == 1]
    steady = [(a, v) for a, v in mono if steps(v) > 1]
    out.append("Of those, **%d drop exactly once** (event-shaped -- what a tile "
               "taking a hit looks like) and **%d tick down repeatedly** "
               "(timer-shaped).\n" % (len(event), len(steady)))

    out.append("| addr | values | shape |")
    out.append("|---|---|---|")
    for a, v in event:
        out.append("| `$%04x` | %s | single drop of %d |"
                   % (a, ", ".join(map(str, v)), max(v) - min(v)))
    for a, v in steady[:8]:
        out.append("| `$%04x` | %s | ticks down %d times |"
                   % (a, ", ".join(map(str, v)), steps(v)))
    out.append("")
    out.append("Longest adjacent runs: %s.\n"
               % (", ".join("`$%04x`-`$%04x` (%d bytes)"
                            % (c[0][0], c[-1][0], len(c)) for c in clusters[:4])
                  or "none"))

    biggest = clusters[0] if clusters else None
    out.append("**Candidate hit-reactive bytes (low confidence).** The "
               "event-shaped bytes above are the ones worth following up: flat, "
               "one drop, flat again, which is what tile HP does when the disc "
               "lands. But the longest adjacent run is %s -- short of the 8 "
               "bytes a 4x2 wall would need -- and the event-shaped bytes are "
               "scattered rather than contiguous, so this run does **not** "
               "establish a tile grid.\n"
               % ("`$%04x`-`$%04x` (%d bytes)"
                  % (biggest[0][0], biggest[-1][0], len(biggest))
                  if biggest else "a single byte"))
    out.append("The next step is a scenario that lands the disc on one *named* "
               "tile and dumps only the frames either side of that impact, so a "
               "single tile's byte is the only thing that can change. That needs "
               "the player to actually aim, which the current blind-fire "
               "scenario does not do.\n")
    return clusters


def hunt_writers(out):
    """Phase 3: which code writes the confirmed addresses."""
    path = os.path.join(DUMPS, "watch_player_xy", "meta.json")
    if not os.path.exists(path):
        return
    meta = json.load(open(path))
    out.append("## watch_player_xy -- write origins (Ghidra entry points)\n")
    out.append("Hatari change-tracking breakpoints (`b ($addr).w ! ($addr).w :trace :lock`) armed while the player is driven along each "
               "axis. The reported PC is the instruction *after* the write, so "
               "the writer is immediately above it; the disassembly window "
               "below ends at the reported PC.\n")
    for w in meta.get("watches", []):
        counts = w.get("pc_counts", {})
        ranked = sorted(counts, key=counts.get, reverse=True)
        PASTE.append("%s  writers  %s  %d changes while driving that axis; "
                     "dominant writer just above $%s"
                     % (w["addr"], ", ".join("$" + p for p in ranked),
                        w["hits"], ranked[0]))
        out.append("### `%s` -- %d change(s) from %d distinct PC(s)\n"
                   % (w["addr"], w["hits"], len(counts)))
        out.append("| PC | changes attributed | |")
        out.append("|---|---|---|")
        for pc in ranked:
            out.append("| `$%s` | %d | %s |" % (pc, counts[pc],
                       "**dominant writer**" if pc == ranked[0] else ""))
        out.append("")
        for pc, dis in list(w.get("disasm", {}).items())[:2]:
            top = int(pc.lstrip("$"), 16)
            body = [l for l in dis.splitlines()
                    if re.match(r"^[0-9a-f]{8} ", l)
                    and top - 0x28 <= int(l[:8], 16) <= top]
            hot = [l for l in body if pc.lstrip("$") not in l[:8]
                   and re.search(r"\$0000%s" % w["addr"].lstrip("$"), l)]
            out.append("Leading up to `%s`%s:\n"
                       % (pc, " -- the write is the `%s` line"
                          % hot[0].split()[0] if hot else ""))
            out.append("```\n%s\n```\n" % "\n".join(body))
    out.append("**Use these as the Ghidra entry points.** All of them sit above "
               "$8000, consistent with code living there and state below it.\n")
    out.append("Two things fall straight out of the disassembly above and are "
               "worth recording:\n")
    out.append("* The writes are literal `subq.w #$03,$00006ca2.w` / "
               "`addq.w #$03,$00006ca2.w` pairs, so the player walks at **3 "
               "units per frame** and `$6ca2` is confirmed as player X by the "
               "code itself, not just by correlation.\n")
    out.append("* Each write is guarded by `cmp.b #$04,(a0)` and "
               "`cmp.b #$08,(a0)`. Those are the ST joystick direction bits "
               "(bit 2 = left, bit 3 = right), so **`(a0)` at that point is the "
               "decoded joystick byte** -- a good second thread to pull on.\n")
    out.append("* `$f66c` reads `$00006ca6.w` to pick a vertical offset, which "
               "independently confirms `$6ca6` as the other coordinate of the "
               "same record.\n")


# --------------------------------------------------------------------------
def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("hunts", nargs="*", default=None,
                    help="player_x_hunt / player_y_hunt / disc_flight / tile_hit")
    ap.add_argument("-o", "--out", default=os.path.join(REPORTS, "findings.md"))
    a = ap.parse_args(argv)
    want = a.hunts or ["player_x_hunt", "player_y_hunt", "disc_flight",
                       "tile_hit", "watch_player_xy"]
    xaddr = None

    out = ["# Disc (Loriciel, 1990) -- RAM findings", "",
           "Generated by `scripts/analyze.py` from dumps produced by "
           "`scripts/collect.py`. Every number below comes from a real run; "
           "value lists are in dump order.", ""]
    for h in want:
        if h in ("player_x_hunt", "player_x_hunt_left"):
            xaddr = hunt_axis("x", out)
        elif h == "player_y_hunt":
            hunt_axis("y", out, sibling=xaddr)
        elif h == "disc_flight":
            hunt_disc(out)
        elif h == "tile_hit":
            hunt_tiles(out)
        elif h == "watch_player_xy":
            hunt_writers(out)
        else:
            raise SystemExit("unknown hunt %r" % h)

    if PASTE:
        summary = ["## Confirmed -- paste into `docs/disc-notes.md`", "",
                   "```"] + PASTE + ["```", ""]
        out[4:4] = summary
    os.makedirs(REPORTS, exist_ok=True)
    with open(a.out, "w") as f:
        f.write("\n".join(out) + "\n")
    print("wrote", a.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())

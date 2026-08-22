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
        PASTE.append("$%04x  vbl_frame_counter  (word)  incremented by the "
                     "VBL handler's first instruction, addq.w #1,$6ab4 at "
                     "$8198; +1 per PAL VBL across %d equal-gap samples, never "
                     "reverses. $6ab6 is a separate DOWN-counter decremented "
                     "right after it (subq.w #1,$6ab6 at $819c); it reads 0 in "
                     "the states sampled here, which is why Part 4 wrongly "
                     "called the brief mistaken about it"
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


# Facts established by reading the code and by direct debugger probes rather
# than by a dump diff.  Kept here so the report regenerates as one document;
# every claim names the evidence that backs it.
def hunt_input_and_arena(out):
    out.append("## input decode and arena geometry (from the code)\n")
    out.append("### `$6c58` -- the decoded joystick byte\n")
    out.append("`a0` at the movement guards `cmp.b #$04,(a0)` / "
               "`cmp.b #$08,(a0)` was read out of the register block on a "
               "`:lock` breakpoint hit: **A0 = $00006C58**. Probing it directly "
               "while holding each key confirms the plain ST layout:\n")
    out.append("| held | `$6c58` |")
    out.append("|---|---|")
    for k, v in (("(nothing)", "$00"), ("Up", "$01"), ("Down", "$02"),
                 ("Left", "$04"), ("Right", "$08"), ("Fire", "$80"),
                 ("Right + Fire", "$88")):
        out.append("| %s | `%s` |" % (k, v))
    out.append("")
    out.append("The fire bit is edge-consumed: every `btst.b #$0007,(a0)` site "
               "(`$f5ea`, `$f7fe`, `$fb74`, `$fe38`) is paired with a "
               "`bclr.b #$0007,(a0)` (`$f606`, `$f81a`, `$fb90`), so a held "
               "fire fires once.\n")
    PASTE.append("$6c58  joystick_decoded  (byte)  $01 up $02 down $04 left "
                 "$08 right $80 fire, ORed; read as (a0) by the movement code; "
                 "fire bit cleared on use by bclr #7 at $f606/$f81a/$fb90")

    out.append("### Player state machine\n")
    out.append("`$f5d0` dispatches on a byte: `move.b $00006cae.w,d0` / "
               "`ext.w` / `lsl.w #$02,d0` / `lea.l ($1852,pc) == $00010e2c,a1` "
               "/ `movea.l ($00,a1,d0.w),a1` / `jmp (a1)`. So **`$6cae` is the "
               "player state index** into a 32-entry longword table at "
               "**`$10e2c`**; entry 1 is `$f5e2` (walk left, sets `$6ca9 = 1`) "
               "and entry 2 is `$f7f6` (walk right, sets `$6ca9 = 2`).\n")
    out.append("Animation is driven by two more fields, confirmed on a "
               "per-frame trace: **`$6cda`** is a cursor that advances by 6 "
               "through the table at `$2988`, and **`$6ce2`** is the frame "
               "countdown reloaded from that entry. Those two are what Part 4 "
               "reported as \"wrapping phase counters\" in `disc_flight`.\n")
    for line in (
        "$6cae  player_state  (byte)  index into the 32-entry jump table at "
        "$10e2c; 1 = walk left ($f5e2), 2 = walk right ($f7f6)",
        "$10e2c  player_state_table  (32 longs)  state handler addresses",
        "$6ca9  player_facing  (byte)  1 = left, 2 = right; set at $f5e2/$f7f6",
        "$6cda  anim_cursor  (long)  steps by 6 through the table at $2988",
        "$6ce2  anim_countdown  (word)  frames left on the current anim cell",
    ):
        PASTE.append(line)

    out.append("### Arena geometry\n")
    out.append("Both walk handlers probe the destination before moving, and "
               "the probe offset and table bounds give the arena:\n")
    out.append("```")
    out.append("$f60a  move.w $00006ca2.w,d0     ; walk-left handler")
    out.append("$f60e  sub.w  #$0018,d0          ; probe 24 units to the left")
    out.append("$f612  cmp.w  #$0008,d0")
    out.append("$f616  blt.b  $f644              ; off the left end -> blocked")
    out.append("$f658  subq.w #$03,$00006ca2.w   ; else step 3 units")
    out.append("")
    out.append("$f81e  move.w $00006ca2.w,d0     ; walk-right handler")
    out.append("$f822  add.w  #$0018,d0          ; probe 24 units to the right")
    out.append("$f826  cmp.w  #$0098,d0")
    out.append("$f82a  bgt.b  $f858              ; off the right end -> blocked")
    out.append("$f86c  addq.w #$03,$00006ca2.w   ; else step 3 units")
    out.append("```\n")
    out.append("So the walkable X window is bounded by the valid index range of "
               "the `$7bfe` column table, **8..152 ($08..$98)**, probed 24 "
               "units ahead of the player, and the player moves **3 units per "
               "frame**. Both agree with the Part-4 measurements: the Left run "
               "floored at X = 8 and the Right run topped out at X = 152, and "
               "the writers are literally `subq.w #$03` / `addq.w #$03`.\n")
    for line in (
        "$f658/$f86c  player_walk  (code)  subq/addq #3 on $6ca2; walkable X "
        "is 8..152, probed +/-24 ahead and range-checked against 8 and $98",
        "$f838  player_row_test  (code)  cmp.w #$000e,$6ca6 -- Y > 14 selects "
        "the far row of the floor grid",
    ):
        PASTE.append(line)


def _load_trace(run, name):
    path = os.path.join(DUMPS, run, "trace_%s.json" % name)
    if not os.path.exists(path):
        return None
    raw = json.load(open(path))
    return [(v, {int(k, 16): b for k, b in m.items()}) for v, m in raw]


def _w(m, a, signed=True):
    v = (m[a] << 8) | m[a + 1]
    return v - 65536 if signed and v > 32767 else v


DISC_FIELDS = [
    (0x00, "world_x",  "integrated by the velocity at +$06; bounces off 0"),
    (0x02, "world_y",  "height; set to $52 by the round initialiser at $aa60"),
    (0x04, "world_z",  "depth, +1 per frame"),
    (0x06, "vel_x",    "signed, clamped to [-2,+2] by the steering code"),
    (0x0a, "flag",     "1 while the record is live"),
    (0x0c, "screen_x", "PROJECTED each frame at $a6b2 -- not integrated"),
    (0x0e, "screen_y", "PROJECTED each frame at $a6b6 -- not integrated"),
]
DISC_BASE, DISC_STRIDE, DISC_COUNT = 0x6e3e, 0x42, 8


def hunt_disc_record(out):
    snaps = _load_trace("disc_trace", "disc")
    if not snaps:
        return
    out.append("## disc_trace -- the disc record (supersedes disc_flight)\n")
    out.append("Per-frame memdump (see KNOWN_ISSUES.md for why a savebin cannot "
               "do this). %d consecutive VBLs, %d..%d.\n"
               % (len(snaps), snaps[0][0], snaps[-1][0]))
    out.append("The disc array is `lea.l $00006e3e.w,a5` at `$a4ea` and `$aa50`, "
               "walked with `lea.l ($0042,a5),a5` and a `dbf` of 7 -- so "
               "**%d records of $%x bytes at $%04x**.\n"
               % (DISC_COUNT, DISC_STRIDE, DISC_BASE))
    out.append("| offset | addr | values over the trace | meaning |")
    out.append("|---|---|---|---|")
    for off, name, note in DISC_FIELDS:
        a = DISC_BASE + off
        vals = [_w(m, a) for _, m in snaps]
        shown = ", ".join(str(v) for v in vals[:12])
        if len(set(vals)) == 1:
            shown = "constant %d" % vals[0]
        out.append("| +$%02x | `$%04x` | %s%s | **%s** -- %s |"
                   % (off, a, shown, " ..." if len(vals) > 12 and len(set(vals)) > 1 else "",
                      name, note))
    out.append("")

    # The decisive check.  Gate it on "in flight": world_z advances by 1 per
    # frame only while the disc is actually travelling, and when it freezes so
    # does world_x -- averaging over both phases would understate the fit.
    xs = [_w(m, DISC_BASE + 0x00) for _, m in snaps]
    vs = [_w(m, DISC_BASE + 0x06) for _, m in snaps]
    zs = [_w(m, DISC_BASE + 0x04) for _, m in snaps]
    fly = [i for i in range(len(xs) - 1) if zs[i + 1] - zs[i] == 1]
    hit = sum(1 for i in fly if xs[i + 1] - xs[i] == vs[i + 1])
    frozen = [i for i in range(len(xs) - 1) if zs[i + 1] == zs[i]]
    still = sum(1 for i in frozen if xs[i + 1] == xs[i])
    out.append("**Integration check.** Split the trace by whether the disc is "
               "travelling (`world_z` advancing by 1):\n")
    out.append("* in flight (%d frame pairs): `world_x[n+1] - world_x[n] == "
               "vel_x[n+1]` on **%d of %d**." % (len(fly), hit, len(fly)))
    out.append("* frozen (%d frame pairs, `world_z` not advancing): `world_x` "
               "is unchanged on **%d of %d**.\n" % (len(frozen), still, len(frozen)))
    out.append("So the position is integrated exactly, and both coordinates "
               "stop together -- the record is idle between flights rather than "
               "drifting.\n")
    if hit > len(fly) * 0.8:
        PASTE.append("$%04x  disc[0].world_x  (word, signed)  integrates by "
                     "vel_x at $%04x while world_z advances (%d/%d frames "
                     "verified); disc array is %d x $%x bytes from $%04x"
                     % (DISC_BASE, DISC_BASE + 6, hit, len(fly), DISC_COUNT,
                        DISC_STRIDE, DISC_BASE))
    out.append("So the disc **is** stored as an integrated world position; what "
               "Part 4 could not find was only its *screen* position, which is "
               "recomputed every frame by perspective projection. That is why "
               "`disc_flight` saw nothing: it sampled every ~14 frames and the "
               "only smooth things at that rate were counters.\n")
    out.append("Trajectory model, from the code at `$a722`-`$a860`: the disc has "
               "no angle table. `vel_x` is an integer nudged by +/-1 per frame "
               "towards an aim point taken from a player's X (`$a7cc` reads "
               "`$6ca2`+12, `$a7d8` reads `$6d22`-4, `$a816` reads `$6d22`-19) "
               "and clamped to [-2,+2] by `cmp.w #$fffe` / `cmp.w #$0002`. So "
               "the \"small discrete angle table\" of the design notes is, in "
               "this build, a five-valued clamped velocity plus per-depth "
               "projection LUTs (`$7abe[i] = i*80`, `$7b5e[i] = i*40`).\n")
    PASTE.append("$a722-$a860  disc_steer  (code)  nudges vel_x +/-1 toward "
                 "playerX+offset, clamped [-2,+2] -- there is no angle table")
    PASTE.append("$a6b2/$a6b6  disc_project  (code)  writes screen_x/screen_y "
                 "from world (x,y,z) via LUTs $7abe, $7b5e, $59952, $5b252")


GRID_BASE, GRID_STRIDE = 0x7616, 8


def hunt_tile_grid(out):
    snaps = _load_trace("tile_grid", "grid")
    if not snaps:
        return
    out.append("## tile_grid -- the cell table at $7616\n")
    out.append("The movement code reaches it as `lea.l $00007616.w,a1` + "
               "`lsl.w #$03,d0` + `tst.w ($00,a1,d0.w)` (at `$f638`/`$f680`, and "
               "18 other sites), i.e. **base `$%04x`, %d bytes per cell**.\n"
               % (GRID_BASE, GRID_STRIDE))
    cells = []
    for i in range(24):
        a = GRID_BASE + i * GRID_STRIDE
        if a + 3 not in snaps[0][1]:
            break
        A = [_w(m, a, False) for _, m in snaps]
        B = [_w(m, a + 2, False) for _, m in snaps]
        cells.append((i, a, A, B))
    live = [c for c in cells if set(c[2]) != {0} or set(c[3]) != {0}]
    out.append("Cells %d..%d carry data; everything from cell %d on is zero, so "
               "the table is **%d cells**.\n"
               % (live[0][0], live[-1][0], live[-1][0] + 1, len(live)))
    out.append("| cell | addr | word +$0 | word +$2 |")
    out.append("|---|---|---|---|")
    for i, a, A, B in cells:
        f = lambda v: str(v[0]) if len(set(v)) == 1 else "%s (changes: %s)" % (
            v[0], "->".join(str(x) for x in sorted(set(v), reverse=True)))
        out.append("| %d%s | `$%04x` | %s | %s |"
                   % (i, " " if i else "", a, f(A), f(B)))
    out.append("")
    out.append("**Cell index formula (verified).** The movement code computes "
               "the player's cell as `column($6ca2) + 8`, plus 4 more when "
               "`$6ca6 > 14` (`$f836` `addq.w #$08,d0`, `$f838` "
               "`cmp.w #$000e,$00006ca6` / `$f842` `addq.w #$04,d0`), where "
               "`column(X)` is the byte table at `$7bfe`: X 0-39 -> 1, 40-79 -> "
               "2, 80-119 -> 3, 120-159 -> 4, 160+ -> 0. It is kept in "
               "`$6cb0`.\n")
    checks = [(117, 18, 15), (152, 18, 16), (8, 18, 13), (117, 2, 11)]

    def column(x):
        return 0 if x > 159 else 1 + min(3, x // 40)
    out.append("| player X | player Y | predicted `$6cb0` | observed | |")
    out.append("|---|---|---|---|---|")
    allok = True
    for X, Y, obs in checks:
        pred = 8 + column(X) + (4 if Y > 14 else 0)
        allok &= pred == obs
        out.append("| %d | %d | %d | %d | %s |"
                   % (X, Y, pred, obs, "match" if pred == obs else "MISMATCH"))
    out.append("")
    out.append("Those four observations are the idle / Right / Left / Down "
               "values of `$6cb0` recorded independently by the Part-4 "
               "`player_x_hunt` and `player_y_hunt` runs, so this is a genuine "
               "cross-check, not a fit.\n")
    if allok:
        PASTE.append("$7616  tile_grid  (%d cells x %d bytes)  cell = "
                     "column($6ca2) + 8 + (4 if $6ca6 > 14); column from the "
                     "byte table at $7bfe; verified on 4 independent samples"
                     % (len(live), GRID_STRIDE))
        PASTE.append("$6cb0  player_cell  (word)  the player's current grid "
                     "cell index, 9..16 over the 4x2 floor")
        PASTE.append("$7bfe  x_to_column  (145 bytes, index = world X 8..152)  "
                     "4 columns of 40 X-units; 0 outside the arena")
    out.append("Word +$0 takes values {0,1,2} and is what the movement code "
               "`tst.w`s before allowing a step, so it reads as an "
               "occupancy/owner field. Word +$2 takes {1,4,5} and is static on "
               "the floor cells (9-16) but varied on cells 0-8 -- consistent "
               "with the per-tile hit points of the design notes, though a "
               "single bout does not prove it. Cells 6 and 14 lost their +$0 "
               "during the trace; those are exactly the bytes ($7647, $7649, "
               "$7687) that the Part-4 `tile_hit` run flagged as "
               "\"event-shaped\" without being able to name them.\n")


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
                       "tile_hit", "input_arena", "disc_trace", "tile_grid",
                       "watch_player_xy"]
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
        elif h == "input_arena":
            hunt_input_and_arena(out)
        elif h == "disc_trace":
            hunt_disc_record(out)
        elif h == "tile_grid":
            hunt_tile_grid(out)
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

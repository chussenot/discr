# Part 12 (bonus) -- the placer named, a bonus placed and picked up on trace

Agent `bonus`. Beads: `discr-ovl.4` (what places a bonus), `discr-ovl.6` (mint
a pickup fixture), `discr-z8m` (the `$6d9a` "damage multiplier").

## Summary

- **discr-ovl.4: CLOSED.** The writer of bit 7 of a tile's hp word, and the
  writer of `$6e3a`, are both named address-for-address and confirmed live.
- **discr-ovl.6: CLOSED.** `tests/fixtures/bonus.ndjson` (152 frames) is
  committed, cross-validated against a same-session Hatari reference for 147
  frames, showing `bonus_6d9a` go non-zero and the `$9aa2`-adjacent pickup
  (bit-7 strip + payload/duration load) exercised.
- **discr-z8m: left OPEN.** `$6d9a`'s writer is now named (`$a2b0`, copying
  whatever the placer minted into `$6e3a`), closing half the bead. The
  multiplier semantics it was actually filed against -- the `$a314 cmp.w
  #1,$6d9a` double-apply -- need a code-1 roll specifically, and every roll
  caught this session (three independent captures) came up code 2. A 20,000
  frame fast-forwarded hunt recording every code seen also came up empty for
  code 1. Documented as a measured limit, not closed.

## Method

### 1. Static analysis first (Ghidra), and where it stopped

`scripts/ghidra/q.sh` against `tmp/ghidra_proj` (the challenge-mode snapshot):
`xref 6e3a` and `scan 9aa2`/`xref 9aa2` reproduced Part 10's decode exactly
(`$a292` tests bit 7, `$a29c` strips it, `$a2ac`-`$a2ca` reads `$6e3a` into
`$6d9a` and the `$9aa2` table into `$6d9e`/`$6d9c`) but found **no writer** of
either `$6e3a` or bit 7 anywhere in the snapshot's instruction model.

Rather than trust that negative on Ghidra's instruction iterator alone, I
wrote a byte-level check against the raw 1MB image (`discram.bin`) for the
literal bytes `76 16` / `75 96` (the two banks' base addresses) anywhere at
all, code or not:

```python
data = open('tmp/ghidra_proj/discram.bin','rb').read()
# find_all(b'\x76\x16'), find_all(b'\x75\x96')
```

38 hits total (21 + 17), every one of them already accounted for by Ghidra's
own instruction list -- so the placer's code is not merely unreached by
Ghidra's function-boundary heuristics, it is a span the batch analysis never
disassembled from any known entry point at all, and does not appear as code
in this snapshot's model in any form. A scan for `$779e` (the tile-collapse
claim-loop's own base address, a completely different mechanism the `tiles`
agent had separately been mapping) turned up exactly one reference, ruling
out the collapse machinery as a placer candidate too.

**Static analysis exhausted; moved to live Hatari**, as the task expected.

### 2. Live change-watch: `scripts/collect.py`'s `watch` step

`scenarios/bonus_hunt.yaml` (mode: challenge, a savestate-cached match,
`{watch: "$6e3a"}` / `{watch: "$6d9a"}` around a scripted rally) found the
writer inside a single run:

```
[watch] $6e3a: 3 hit(s) from PC $9d48, $a2b4, $a2ce
[watch] $6d9a: 3 hit(s) from PC $9d48, $a2b4, $a2ce
```

`collect.py`'s `watch()` arms a Hatari change-tracking breakpoint per address
but `unwatch()` scans the whole log slice since that watch was armed for
*any* `CPU=` line, not just the addresses that specific breakpoint owns --
so with both watches armed for the whole scenario, each report is really a
merge of both conditions' hits. Disassembling each reported PC resolved it
cleanly anyway: `$9d48` is the instruction after `$9d42 move.w #2,$6e3a`
(a genuine `$6e3a` write); `$a2ce` is the instruction after `$a2ca clr.w
$6e3a` (the known consume-clear); `$a2b4` is the instruction after `$a2b0
move.w D0w,$6d9a` (the known consume-copy). One real placer write, one real
clear, one real copy -- all three already made sense once read against the
disassembly, and the placer PC (`$9d42`, inside a span with no defined
function) was the new fact.

### 3. Following the trigger up

Disassembling `$9c9a`-`$9d88` (a standalone script driving Hatari's debugger
directly, not through a scenario file, since I needed arbitrary `disasm`
windows) found the full gate: `$6e3c`, a countdown reloaded to `$14`=20 every
time it expires, and only then a PRNG roll (`$6c5d += $6ab5`, masked `&
$7f`) that either does nothing (117/128) or writes one of five codes into
`$6e3a` (11/128) before falling through to `$9aea`, which rolls again (same
PRNG, `& 7`) to pick one of 8 eligible cells and `or.w #$0080` bit 7 into
both banks at once. Full addresses and the exact bucket boundaries are in
`docs/disc-notes.md`'s new Part 12 (bonus) section; not repeated here.

### 4. Minting the fixture

`tmp/mint_trace3.py` (kept local, not committed -- scratch driver, same shape
as `scripts/oracle_diff.py`'s own `hatari_side()`): poll `$6e3a` idle from
the cached challenge savestate every 2 frames; the instant it goes non-zero,
capture a seed (`h.seed()`) and immediately continue tracing the same live
session (`h.frame_trace()`) -- no intervening debugger round trips, so the
just-placed cell cannot be destroyed or consumed before the seed lands.
Reproducibility was checked directly: two independent runs of this recipe
produced byte-identical seeds (same sha256), because the machine is
deterministic from the savestate and idle input carries no wall-clock jitter
into emulated state.

`scripts/oracle_diff.py --seed tmp/bonus_diff.seed --cache
tmp/hatari_ref_bonus.json --frames 250` (the cache holding the Hatari side
captured in the same session as the seed) reported:

```
aligned on $6ab4=6416: oracle frame 5 == Hatari trace index 0
FIRST DIVERGENCE at frame 147 ($6ab6, $6c5d, ...)
TIER 1 (frame-exact): 147 frames.
```

Working out the alignment: the bonus placement/pickup transition
(`tiles[7].hp` 132 -> 4, `bonus_6d9a` 0 -> 2) happens at oracle frame 151,
which is Hatari-aligned frame 146 -- the LAST frame inside the 147-frame
tier-1 window, not past it. **The one event this fixture exists to prove is
inside the validated region**; the divergence at the next frame is a
different, unrelated fact (see below).

`./oracle/disc-oracle --seed tmp/bonus_diff.seed --script tmp/idle.script
--frames 152 --trace tests/fixtures/bonus.ndjson` produced the committed
fixture, trimmed to the Hatari-validated length.

`cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/bonus.ndjson
--skip-waived` measured disc-core's own agreement against it: **150 ticks**,
diverging exactly at the pickup (`tiles[7].hp`: ST/oracle `132 -> 4`,
disc-core `132 -> 129` -- a plain damage subtraction, oblivious to bit 7).
Without `--skip-waived`: **22 ticks** (player 2's AI enters an unhandled
`$c6ec` state, the same pre-existing `discr-b6x` limitation `golden.ndjson`
and `tile_damage.ndjson` already waive around -- not new).

### 5. Hunting code 1 for discr-z8m

`tmp/hunt_code1.py`: fast-forwarded, polling every 4 frames for up to 20,000
frames, logging every code seen (not just the first), stopping the instant
`$6e3a` reads 1. Result: two rolls seen (frame 8 and frame 88 of that run),
both code 2, then nothing further -- not because the gate stopped firing,
but because a change-detector that only logs `$6e3a` when it *differs from
its last-seen value* cannot see a second roll landing on the same code while
the first is still unconsumed (`$6e3a` is never reset to 0 except by a
pickup; a failed roll leaves it untouched, and a successful reroll of the
same code looks like no change at all). A genuinely rarer code sitting behind
a long unconsumed streak would still have been caught, since 1 != whatever
was last seen -- so the honest reading is: this specific 20,000-frame window
did not roll code 1, full stop, not that the search missed it.

## The oracle-vs-Hatari divergence past frame 147, briefly

`$6c5d` disagrees at the very first diverging frame (Hatari `$53` vs oracle
`$df` -- a different PRNG state, not an off-by-one). `$6c5d` is exactly the
byte this bonus roll advances on every gate-fire, and this is the first
fixture that ever exercises that code path at all (every prior trace had
`bonus_6d9a` = 0 throughout, so the roll's outcome was never load-bearing
before now). The likely mechanism is the project's own previously-documented
category (Part 11f-g): Musashi is instruction-accurate, not cycle-accurate,
and this game's main loop runs a variable number of passes per VBL -- if the
`$6e3c` gate is serviced once per pass rather than once per VBL, a
one-pass-per-VBL assumption anywhere upstream desyncs `$6c5d` the instant the
pass count first differs. Not chased further: it is a `discr-b6x` (PRNG hunt)
question, and it sits after the event this fixture exists to prove.

## Files

- `tests/fixtures/bonus.ndjson` (`git add -f`, gitignored like every other
  `*.ndjson`) + `tests/fixtures/bonus.provenance.md`
- `mise.toml`: `BONUS`, `BONUS_MIN_AGREE=150`, `BONUS_FULL_MIN_AGREE=22`,
  wired into `core-check` and `tracecheck`
- `docs/disc-notes.md`: one appended "Part 12 (bonus)" section
- `scenarios/bonus_hunt.yaml` (committed; the live change-watch scenario)
- Not committed (scratch, local to this worktree's `tmp/`, gitignored):
  `tmp/mint_seed.py`, `tmp/mint_seed2.py`, `tmp/mint_trace3.py`,
  `tmp/hunt_code1.py`, `tmp/probe_placer.py` -- kept for whoever chases
  code 1 next; the recipe in section 4/5 above is enough to rebuild any of
  them from scratch if they are not present in a fresh worktree.

## Handoff: what would close discr-z8m

Re-run `tmp/hunt_code1.py`'s recipe (or similar) until `$6e3a` reads 1, mint
a seed the same way, confirm `$a314`/`$a31c` fire (the tile's damage
subtracted twice in one hit), and implement it in `tile::damage` -- the
signature change needed is a `bonus_code: u16` (or similar) parameter, which
means a one-line call-site update in `disc.rs` (owned by `hook` this round)
alongside the `tile.rs` change. Coordinate via a wait-lease on `disc.rs` or
hand the diff to the file's next owner; do not guess the plumbing without a
code-1 trace in hand to check it against.

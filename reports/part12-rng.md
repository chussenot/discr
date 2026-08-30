# Part 12b — `$6c5d` live: found the seed, still can't use it

The brief (bd discr-b6x) was to attack `$6c5d` *live* where `reports/part12-ai.md`
could only argue from static disassembly, and to transcribe the twelve-entry
walk cascade (rows 6-17) that report left undecoded. Both are done as far as
the evidence goes; neither cashes in the waiver. What follows corrects one
claim in Part 12a: **`$6c5d` is reset** — Part 12a said it never was, and that
was wrong, found only by going around the tool that produced it.

## The method flaw Part 12a didn't hit, and how this phase found it

Part 12a's `$6c5d` writer search was `./scripts/ghidra/q.sh xref 6c5d` —
Ghidra's cross-reference index, built from whatever code its auto-analysis
already disassembled. That index reported exactly four read+write site pairs,
all the same `x = (x + $6ab5) mod 256` pattern, and *no* reset — a real
finding as far as it went, but silently bounded by Ghidra's own analysis
coverage, the same blind spot Part 12a itself had already hit once, on
`$efa8` (a data table `dis`/`dec` silently misread as whatever code follows
it). This phase re-ran the check a different way: a raw byte-pattern scan of
`discram.bin` for the literal operand bytes `6c 5d`, independent of whether
Ghidra ever disassembled the surrounding code as instructions at all.

    python3 -c "
    with open('discram.bin','rb') as f:
        data = f.read()
    pat = bytes([0x6c,0x5d]); i = 0; offs = []
    while True:
        i = data.find(pat, i)
        if i == -1: break
        offs.append(i); i += 1
    print(len(offs), offs)"

19 hits, not 8. Eight of them are the four pairs Part 12a already had
(`$d2fa`/`$d302`, `$da0e`/`$da16`, `$df8c`/`$df94`, `$d07a`/`$d082` — the byte
offsets line up exactly once the 2-byte lead-in of each `move.b`/`add.b`
opcode is accounted for). The other eleven are in code Ghidra's `xref`,
`dis`, and `dec` all show nothing for — `./scripts/ghidra/q.sh xref 9682`
(the block below) returns **zero references**, and `dis 9640 20` doesn't even
land at `$9640`; it silently prints instructions starting at `$9938`, the
exact "falls forward to whatever real instruction follows" failure mode Part
12a already named for `$efa8`, just somewhere Part 12a's `$6c5d`-specific
search never poked.

## `$968a`: the reset

Reading the raw bytes at the first unexplained cluster, `$9680`-ish, by hand
(same technique Part 12a used for `$efa8`'s table — cross-checked against
every neighboring instruction's own known encoding, e.g. `11c0 6c5d` = `MOVE.B
D0,($6c5d).w`, already confirmed at `$d302`):

```
$9682  clr.b  $6c83
$9686  clr.w  $6c9c
$968a  move.b $6ab5,$6c5d      ; <-- THE RESET. Unconditional. Direct copy,
                                ;     not an accumulate -- opcode $11f8, both
                                ;     operands absolute.w ($6ab5 source,
                                ;     $6c5d dest), 6 bytes, no register used.
$9690  clr.w  $6ab8
$9694  clr.b  $6c5a
$9698  move.w #0,$6ab6
$969e  bsr.w  $9e7a
$96a2  bsr.w  $ab98
$96a6  bsr.w  $a4a0
$96aa  bsr.w  $9c44
$96ae  bsr.w  $a1d4
$96b2  bsr.w  $a0f2
$96b6  bsr.w  $a4bc
$96ba  move.w $6ab8,-(sp)
$96be  bsr.w  $a4ea
$96c2  bsr.w  $10eac
  ...
```

This settles the "never reset" claim in Part 12a's own terms: it's false,
address-cited. `$6c5d` is reseeded, unconditionally, from `$6ab5` — the same
VBL-frame-counter low byte Part 12a already correctly identified as the
roll's stride (`$6ab4`, incremented once per VBL by the interrupt handler at
`$8198`, confirmed again this phase: still exactly 3 references to `$6ab4` in
the whole image, one write, and it is the *only* write anywhere).

**What this phase could not pin down**: what calls this block. `xref 9682`
is empty (Ghidra never disassembled it, so it has no caller graph either), a
direct scan for `JSR`/`JMP #$9682` and for `BSR` displacements landing in
`$9670`-`$96d0` across the whole image found nothing — meaning, like the
AI table and two of the six walk-cascade tests below, it's reached only
through an indirect (table-driven) call this phase's tools don't resolve.
So *whether* this runs once per match, once per round, or once per serve is
still open; only that it runs, unconditionally, from *some* trigger, is
proven.

## Live check: the two things that matter, both measured

`scenarios/watch_6c5d_rng.yaml` (leased, committed): reach a live training
match, `dump` a full `$0`-`$8000` savebin before any scripted input, then two
`trace` windows (per-VBL `$6ab4..` snapshots, which land `$6c5c`/`$6c5d`/
`$6c5e` at a fixed +0x1a8 offset in the same dump — one trace pass gets both
the ground-truth frame counter and the target byte with no second pass).
Run twice, both `--fresh --state ''` (a real cold boot each time, no
savestate cache, so the only thing shared between runs is the disk image and
the scripted input timing):

```
python3 scripts/collect.py --scenario scenarios/watch_6c5d_rng.yaml \
    --dumpdir /tmp/rng-run1 --state '' --fresh
python3 scripts/collect.py --scenario scenarios/watch_6c5d_rng.yaml \
    --dumpdir /tmp/rng-run2 --state '' --fresh
```

**1. `$6c5d` is already nonzero before any scripted rally input.**
`pre.bin` (the savebin taken right after `settle: 90`, before the scenario's
own `hold: Fire`) reads `$6c5d = 0xbc` (188) in run 1, matching the trace's
own first sample bit-for-bit — so whatever reseeds it at `$968a` already ran
during boot/menu navigation, well before this scenario's own first
scripted button press, confirming it's not a serve-time-only mechanism this
scenario's own inputs would trigger fresh.

**2. Two boots, identical script, different everything.** The game's own
16-bit VBL counter (`$6ab4:$6ab5`, read back by the harness the same way
`Part 11`'s oracle-seed contract does) at the first post-settle sample:

| run | game VBL at first sample | `$6c5d` | `$6c5d` after idle window | `$6c5d` after rally window |
|---|---|---|---|---|
| 1 | 4356 | 188 | 30 (at frame 94 of 189) | 132 (unchanged through 189) |
| 2 | 4235 | 4 | 10 (at frame 123 of 189) | 10 -> 20 (at frame 12 of 189) |

The two boots' own frame counters disagree by 121 VBLs by the time a match
goes live, despite running the *exact same* scripted button-tap sequence
(`[nav] menu reached after 4 space taps`, `3 fire(s)` both runs) — meaning
the boot-to-match-live path is not frame-reproducible even under this
project's own deterministic-looking harness, before `$6c5d` even enters the
picture. `$968a`'s reseed then copies whatever `$6ab5` happens to be *at
that already-divergent instant*, so the two runs' `$6c5d` sequences share no
value at any matching scenario step.

**3. Cadence confirms Part 12a's "not every VBL" prediction, live.** `$6c5d`
sits perfectly flat for tens of frames (94 of them in run 1's idle window)
then jumps by a large amount in one step — consistent with Part 11f/g's own
finding that the update pass carrying `$d2cc` doesn't run every VBL, and
with the dispatch loop's structure (`$d2ec`/`$d2f2`): with nothing latched
(`$6daa == 0`), *every* row's priority (minimum 8) clears the "priority >
latch" gate, so a single pass can burn on the order of twenty rolls at once
— exactly the kind of jump a flat-then-jump trace shows, not a steady +1.

## Verdict: the wall is real, and now it's a sharper wall

Part 12a argued reconstruction was circular (the roll's cadence depends on
latch history, which depends on earlier rolls). That's still true, but this
phase's finding replaces "and also it never resets, so there's no anchor at
all" with something more precise and, on balance, still a wall: **there is
an anchor (`$968a`), but the anchor's own input — `$6ab5`, i.e. elapsed VBLs
since a cold boot with no reset of its own — is exactly the quantity this
phase measured as not reproducible across two identical scripted runs.**
Knowing the reset instruction doesn't help unless a fixture also captures
`$6ab4`/`$6ab5` at the moment `$968a` last ran before the fixture's capture
window starts, and no fixture on hand does (nor could any fixture generated
by this harness be trusted to hit the same VBL twice, per the table above).
So: not reconstructable *after the fact* from any fixture this project has,
same conclusion as Part 12a, but now for a cause that's measured rather than
argued, and with one wrong sub-claim ("never reset") corrected.

**What would close it**: a fixture whose frame 0 is anchored to the *serve*
event with `$6ab4`/`$6ab5` sampled in the same record (the existing `banks`/
`disc` fields already show the pattern — one more fed pair), *and* a live
answer to what calls `$968a` (round start vs. every serve matters for
whether a serve-anchored fixture would actually catch a fresh reseed or an
already-stale value). Both are now precisely scoped follow-ups, not "figure
out the whole thing again."

## Rows 6-17: the walk cascade, as far as this phase got

The table (`reports/part12-ai.md`) already has all twelve rows' addresses:
six position tests (`$dd68`, `$de8e`, `$de12`, `$ddd4`, `$ddc4`, `$da84`),
each paired with one of two actions (`$deea`, `$df58`), sharing identity
`$e274`.

**The two actions are fully decoded.** Both are the same "plan compiler"
pattern Part 12a documented for rows 0/1's `$e214`, just choosing between two
step types by comparing `d4`/`d2` (candidate cell indices from whichever test
just ran) against each other:

```
$deea  cmp.w d2,d4
       beq -> plan = [$e30a, target(d4)]         ; same cell: walk-to-target
       else -> plan = [$e37a, target(d4), $e30a, target(d2)]  ; TWO steps:
                                                   ; do $e37a first (an
                                                   ; unexplored step type,
                                                   ; not $e30a/$e2d0's walker),
                                                   ; then walk to target(d2)
       ; then: if D7's low nibble is nonzero, append a THIRD step via
       ; $e2b4 (also unexplored) carrying that nibble | $80 and a 25-frame(?)
       ; timeout ($0019); if D7's low nibble is zero, append two $e2b4 steps
       ; with fixed literal params ($8100/1, $8000/$19) instead.

$df58  identical shape to $deea, except its "same cell" case ALSO uses
       $e37a (not $e30a) -- so $df58's target(d4)==target(d2) case does not
       just walk there, it does whatever $e37a does twice in a row.
```

Both index the *same* `$15fe` table Part 12a already fully transcribed (the
escape/avoid target centers), so `target(n)` is exactly `ESCAPE_TARGET[n]` in
`ai.rs`'s terms — no new table needed there. What's genuinely new and
unresolved: `$e37a` and `$e2b4`, two step types `$e30a`'s per-frame executor
(`$e290`) would dispatch just like it does `$e30a`, never decoded because
rows 0/1 never compile them. Until those two are read, `$deea`/`$df58`
cannot be turned into `ai.rs` code without guessing what a large fraction of
their own output does — exactly the thing the house rules ask not to do.

**Four of the six tests are at least structurally readable**; two are not
in Ghidra's instruction database *at all*:

| test | Ghidra has it? | shape |
|---|---|---|
| `$dd68` | yes | bounds check (`$6d8a` vs `$6d8c`, own-Y-vs-something) -> `bsr $d0e2` (checks flag `$6e16`, returns fixed cell `$6e36`) -> success |
| `$de8e` | yes | bounds check -> builds an 8-bit occupancy mask from `$1556`/`$155e` (the SAME escape-direction tables Part 12a transcribed) filtered through `$d0b0` (a near-bank, `$761e`, HP-nonzero probe) -> first candidate direction that clears both checks wins, falling back to `$dd2c` on total failure |
| `$ddc4` | yes | bounds check -> `bra $dd2c`, a *third* shared helper (own grid cell minus 9, straight into `$d0b0`, unconditionally, no direction search) |
| `$ddd4` | yes | bounds check -> tries four fixed direction codes (1, 2, 5, 6) in order via `$dd34` (`$6e14 = code; bsr $d0b0; success/fail`), first success wins |
| `$de12` | **no** — 4 xrefs, all `DATA` (referenced only as a function pointer, from `$ea98`/`$ee60`/`$ef60`/`$f044` — other rule tables besides `$efa8`, not yet identified) | partial hand-read only: bounds check (same shape), then reads player 1's own `$6ca2`/`$6ca6` (world_x/world_y, per `scenarios/watch_player_xy.yaml`'s own labels) and a `cmp.w #0x14,D0; ble +0x16` before a `bsr` this phase did not resolve — stopped here rather than guess the branch target from hand-arithmetic on a raw byte dump with no second source to check it against |
| `$da84` | **no** — 6 xrefs, all `DATA` | not read at all this phase (budget) |

**The shared helper web** (all address-cited, several read this phase for
the first time): `$d0b0` (near-bank, `$761e`, HP probe with a `$6d9a==3`
special case at cell 7 — decoded fully, reused by `$dd2c`/`$dd34`/`$de8e`),
`$d07a` (one of the four *original* `$6c5d` roll sites — a masked-to-0..7
near-bank probe, its own retry loop), `$d0e2` (flag check, `$6e16`), `$d0f4`
(an 8-slot near-bank occupancy scan building a priority-ordered bitmask,
structurally a twin of `$d0f4`'s sibling `$d17e` which does the same thing
against the *far* bank `$759e` instead of `$761e` — both fully read, neither
finished into a case-by-case model), `$dbfc`/`$db54`/`$dba8`/`$daf8` (a
multi-candidate scorer that picks between two slots by X-distance to
player 2's own position — partially read, not finished).

**Not implemented in `ai.rs` this phase.** Two of six tests aren't even in
Ghidra's database yet (needing the same raw-byte archaeology this phase used
for `$968a`, just not finished for these two), and the two actions both
depend on two wholly unread step types (`$e37a`, `$e2b4`). Writing Rust for
what's left would mean inventing behavior for gaps this size — exactly what
the house rules ask not to do to move a number. `ai::agreement`'s numbers are
therefore unchanged this phase: golden 18/99, tile_damage 61/214, p1_walk
22/200 (re-run, confirmed identical: `cargo test -p disc-core --lib
ai::agreement -- --nocapture`).

## The knockdown anomaly: unchanged

No new evidence this phase bears on it. `docs/disc-notes.md`'s existing
account (four `p1_walk` ticks at player 2's state-11 transition, row 1
latching and steering when the ST's own byte says otherwise, neither
exclusion list naming state 11) stands as written; nothing decoded this
phase touches `$e158`, `$e30a`, or the state-11 transition itself. Left open,
same as Part 12a left it.

## Files

* `scenarios/watch_6c5d_rng.yaml` — the two-boot cross-check scenario; re-run
  with `--fresh --state ''` (twice, for two independent boots) to reproduce
  the divergence table above.
* No changes to `crates/disc-core/src/ai.rs` this phase — nothing decoded
  here reached a state safe to implement without guessing. `mise run
  core-check`'s five gates, and `ai::agreement`'s three pinned numbers, are
  unaffected.

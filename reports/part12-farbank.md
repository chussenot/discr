# Part 12 (farbank): the coordinated PlayerId flip, and the far bank compared

bd discr-ovl.8, then bd discr-ovl.3. Every claim below ties to an ST address,
a citation already in `docs/disc-notes.md`, or a command run in this
worktree.

## discr-ovl.8: the flip inventory

Part 12 (owner) pinned raw `disc+$11` == 0 to PLAYER 2's disc, 0xFF to PLAYER
1's, and filed this bead because `disc-core`'s internal `disc.aim ==
PlayerId::One` checks were written under the OPPOSITE, self-consistent
convention (raw 0 <-> `PlayerId::One`). A single-arm flip (only `main.rs`'s
feed) was tried and reverted before this bead existed: it regressed
`p1_walk` 274 -> 10 because it desynced the feed from disc.rs's/player.rs's
own checks, which were never compared against the feed before (`aim` is fed
every tick, never compared).

The fix landed here is coordinated: every site found by grepping the enum
and following the compiler through the change, flipped in one commit.

| file:site | before | after | reasoning |
|---|---|---|---|
| `main.rs::seed()`, `aim` feed | `Some(0)\|None => One` | `Some(0)\|None => Two` | raw 0 is real player 2's disc |
| `disc.rs::step`, retire-check `holder` | `if aim==One {1} else {0}` | `disc.aim.index()` | the hand-inversion this line used to reach the real holder despite the old convention is exactly what the flip lets go |
| `disc.rs::step`, near wall force/transfer | `if aim==One` | `if aim==Two` | raw 0 (real player 2's serve) forces `dir_kind`+damages the near grid (`$a618`) |
| `disc.rs::serve`, served disc's `aim` | `PlayerId::One` | `PlayerId::Two` | every serve is unconditionally owner 0 (`$a9bc`), charged to player 2 |
| `player.rs::hit_test`, player 1's dock | `if aim==One` | `if aim==Two` | raw 0 docks player 1 (`$1116e`) |
| `player.rs::p2_hit_test`, catch window | `if aim==One` | `if aim==Two` | raw 0 (still player 2's own serve) opens player 2's catch states |
| `player.rs::strike`, player 2's dock | `if aim!=One` | `if aim!=Two` | raw non-zero docks player 2 (`$c9a6`) |
| `player.rs::anticipate`, `theirs` | two hand-tuned match arms | `disc.aim == who` | both arms were compensating for the same One/Two mismatch `holder` was; the flip collapses them to one comparison |

Nine sites total (eight pre-existing + the new far-wall branch, discr-ovl.3's
half, using the correct polarity directly since it's new code -- see below).
Every one is a **literal swap of the `PlayerId` literal compared against
`disc.aim`**; no other logic changed, which is exactly why the fixture
numbers hold at their pre-flip values rather than needing new tolerance.

`docs/state-schema.md`'s `discs[n].aim` row prose is updated citing this
closure; the row itself is unchanged (`waived:discr-ovl.2`, fed not
compared -- `disc-core` still has no writer for the field).

### Why `handover.ndjson` is the proof, not a coincidence

`handover.ndjson` is the only committed fixture whose disc slot visits BOTH
raw owner values on the same slot (frame 259: 0 -> 255 at the far wall;
frame 339: 255 -> 0 at the near wall). A wrong flip has two ways to fail
here: either arrival forces the wrong `dir_kind` magnitude (the near wall's
own comment already flags this trap -- forcing `+1` is not `neg.w` of a `-3`
return leg, which would give `+3`), or a wrong `holder`/`theirs` computation
picks the wrong player's counters/cascade. Both `HANDOVER_MIN_AGREE = 21`
and `HANDOVER_SKIP_MIN_AGREE = 222` are unchanged after the flip, which is
the fixture doing its job.

## discr-ovl.3: the far bank, compared

**Before this bead**: `disc-core` carried `tiles_far` (from the four-slot
collapse work) and the oracle emitted `banks` -- both bank's 16 cells each,
32 pairs, since Part 10e -- and `main.rs::seed()` already zipped the first
16 into `tiles_far`. Nothing ever *compared* it: `tracecheck`'s `checks()`
had no `tiles_far[n]` rows, and `scripts/oracle_diff.py`'s labeller fell
through to "unlabelled" for `$7596..$7616` even though the differ's window
(`$6a00-$76a0`) already covered those bytes.

**What landed**:

1. `checks()` gains `tiles_far[n].tile_type`/`tiles_far[n].hp`, iterating
   `expected.banks.iter().take(TILE_CELLS)` zipped against `got.tiles_far` --
   an empty `banks` (a pre-Part-10e trace) makes the loop a no-op rather than
   comparing against a phantom column, and `not_in_trace` reports the two
   rows by name in that case (the same shape `discs[n].damage` already uses
   for `dmg`).
2. `SCHEMA_COMPARED` 18 -> 20, `SCHEMA_WAIVED` 17 -> 15;
   `projection_is_never_compared`'s instance-count formula gains a second
   `TILE_CELLS * 2` term (golden.ndjson already carries `banks`, so the new
   rows are live in that test's own trace).
3. `docs/state-schema.md`: the two far-bank Waived rows (`-- (the far wall's
   tile grid)` and `-- (the far bank $7596)`, both `waived:discr-ovl.3`) move
   to Compared as `tiles_far[n].tile_type`/`tiles_far[n].hp`; prose updated
   in both the Compared-fields notes and the "Why each waiver" bullet.
4. `scripts/oracle_diff.py`'s `label_for` gains a `$7596..$7616` case,
   mirroring the existing `$7616..$7696` one.
5. `disc::step`'s far-wall branch (the only genuinely new CODE, as opposed to
   plumbing): `$a5d6`/`$9f5e` mirrors the near wall's `$a618`/`$a24c`
   exactly. The cell-index formula is shared verbatim -- "`$9f5e` is `$a24c`
   instruction-for-instruction" (docs/disc-notes.md, Part 10) means the
   substitution (`lea $7596` for `$7616`, `$6d1c` for `$6d9a`) touches only
   the bank base and the bonus-code word, not the `d0`/`d1` column
   computation `disc_cell` already implements -- so the far-wall branch
   calls the SAME `disc_cell`/`impact` disc-core already had, against
   `tiles_far` instead of `tiles`. A new constant, `FAR_WALL_DIR_KIND = -1`
   (`-SERVE_DIR_KIND`), and a new unit test,
   `the_far_bound_damages_the_far_banks_cell_the_disc_is_over`, mirroring the
   near-wall test address for address.

### The fixture: `tests/fixtures/farbank.ndjson`

295 idle frames, fresh CHALLENGE seed (sha256
`c3d4554801acd7a003b57e25f8cc428f0954c871145159af822bb81e3badc51a`, `$6ab4`
= 6312). Confirms the far bank's frame-0 seed matches the ST's own memory for
16 non-trivial cells (not placeholder zeros) and stays matched for 34 ticks,
until both banks' cell 7 lose a bonus flag (bit 7) in lockstep at frame 35 --
the same placer (`$9b28`/`$9b32`, already named for the near bank) writing
both banks' same slot, confirmed live rather than only from the disassembly.
`disc-core` has no bit-7 model for EITHER bank (discr-dc0/discr-ovl.4's
existing gap), so it diverges there, reported on the near bank's
pre-existing row since schema order checks it first -- `tiles_far[7].hp`
does not disagree any earlier. `FARBANK_MIN_AGREE = 34` in `mise.toml`, both
modes (this capture's player 2 rows do not diverge before tick 34 either).
Full recipe, a second bonus event at frame 282, and the fixture's own
disc-lifecycle detail: `tests/fixtures/farbank.provenance.md`.

**What it does not reach**: a genuine far-wall DAMAGE hit needs a disc
already owned by real player 1 to cross the far wall a SECOND time, which
needs player 1 to bounce it back before it reaches the near wall (arriving
there transfers possession back unconditionally, closing the window). Three
measured attempts (idle across three fresh seeds; a scripted approach that
landed player 1 within single digits of the disc's X at the crossing frame
with no bounce resulting; a closer approach that overshot into an unrelated
arena-edge state) all failed to land it, and all hit the same practical
ceiling regardless of input: an unstubbed floppy/PSG access around frame
460-500, consistent with a round-transition disk load outside the oracle's
built stub list. The far-wall damage branch is therefore modelled from the
disassembly and unit-tested directly, but **untested by any committed
fixture** -- the same shape as discr-ovl.1's player-1 racket path (closed as
"decoded, not fixture-exercised" since neither player ever swings in either
fixture). Attempt log and a recipe for whoever picks this up:
`tests/fixtures/farbank.provenance.md`.

## Gates

```
cargo fmt --check                                                          clean
cargo clippy --all-targets -- -D warnings                                  clean
cargo test                                                                  65 passed (54 disc-core, 11 tracecheck)
cargo clippy -p disc-app --all-targets -- -D warnings                      clean
cargo test -p disc-app                                                      5 passed

tracecheck golden.ndjson       --skip-waived --min-agree 99   -> OK, 99   (unchanged)
tracecheck tile_damage.ndjson  --skip-waived --min-agree 214  -> OK, 214  (unchanged)
tracecheck golden.ndjson       --min-agree 99                 -> OK, 99   (unchanged)
tracecheck tile_damage.ndjson  --min-agree 214                -> OK, 214  (unchanged)
tracecheck p1_walk.ndjson      --min-agree 274                -> OK, 274  (unchanged)
tracecheck handover.ndjson     --min-agree 21                 -> PASS, 21  (unchanged)
tracecheck handover.ndjson     --skip-waived --min-agree 222  -> PASS, 222 (unchanged)
tracecheck bonus.ndjson        --skip-waived --min-agree 150  -> PASS, 150 (unchanged)
tracecheck bonus.ndjson        --min-agree 22                 -> PASS, 22  (unchanged)
tracecheck farbank.ndjson      --min-agree 34                 -> PASS, 34  (new gate)
```

All nine pre-existing gates hold at their exact pre-flip numbers; the tenth
is new. No number moved except by addition.

## Files touched

* `crates/disc-core/src/disc.rs` -- the flip (`holder`, near-wall condition,
  `serve`'s `aim`), the far-wall branch, `FAR_WALL_DIR_KIND`, `Z_FAR`'s and
  `disc_cell`'s doc comments (both were stale -- `Z_FAR` claimed no trace
  reaches it; `handover.ndjson` frame 259 already did), `step`'s own doc
  comment, two test fixups (`aim: PlayerId::Two` added where the near-wall
  force branch is under test), one new test.
* `crates/disc-core/src/player.rs` -- the flip (four sites), `anticipate`'s
  `theirs` simplified to `disc.aim == who`.
* `crates/disc-tools/src/main.rs` -- the feed flip and its comment rewrite,
  `tiles_far[n]` compared rows in `checks()`, `not_in_trace`,
  `SCHEMA_COMPARED`/`SCHEMA_WAIVED`, `projection_is_never_compared`'s count.
* `docs/state-schema.md` -- two rows Waived -> Compared; owner-polarity and
  far-bank prose both updated; waived/excluded totals recomputed.
* `docs/disc-notes.md` -- one appended section, "Part 12 (farbank)",
  covering both beads.
* `scripts/oracle_diff.py` -- `label_for` gains the far-bank case.
* `oracle/disc-oracle.c` -- comment only.
* `mise.toml` -- `FARBANK`/`FARBANK_MIN_AGREE`, wired into `core-check` and
  `tracecheck`; `core-check`'s own description corrected (five runs over
  three fixtures -> ten over six, already stale before this bead).
* `tests/fixtures/farbank.ndjson` (new, `git add -f`) + `.provenance.md`.
* This file.

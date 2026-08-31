# farbank.ndjson -- provenance

bd discr-ovl.3. 295 frames, idle. The first fixture minted specifically to
confirm the far bank ($7596, `disc_core::GameState::tiles_far`) against a
live trace, rather than only seeding it and never checking it.

    python3 scripts/collect.py --scenario scenarios/oracle_seed.yaml
    ./oracle/disc-oracle --seed seeds/match_challenge.seed --frames 295 \
        --trace tests/fixtures/farbank.ndjson

* **Seed**: `seeds/match_challenge.seed` (gitignored, like every other seed),
  sha256 `c3d4554801acd7a003b57e25f8cc428f0954c871145159af822bb81e3badc51a`,
  captured at PC == `$8198` in a live CHALLENGE round (`$6ab4` = 6312), via
  `scenarios/oracle_seed.yaml` -- the same recipe `tests/fixtures/
  handover.provenance.md` and `bonus.provenance.md` use.
* **Input**: none. Idle, like `tile_damage.ndjson` and `bonus.ndjson` -- the
  events this fixture is cited for are the ST's own timer-driven bonus roll
  and the opponent's own play; no input programme was needed to provoke
  either.

## What the fixture confirms: the far bank reads and compares clean

`disc-oracle` has emitted `banks` (both 16-cell tile banks, `$7596` then
`$7616`, 32 `[tile_type, hp]` pairs) since Part 10e; `crates/disc-tools/
src/main.rs`'s `seed()` has zipped the first 16 of those into
`GameState::tiles_far` since the same part. Neither `disc-core` nor
`scripts/oracle_diff.py` had ever *compared* it -- discr-ovl.3's own opening
line. This bead adds `tiles_far[n].tile_type`/`tiles_far[n].hp` as compared
rows in `checks()` (gated on `!expected.banks.is_empty()`, reported via
`not_in_trace` on an older trace that lacks the column) and gives
`scripts/oracle_diff.py`'s labeller a case for `$7596..$7616` (the window
already covered those bytes; they fell through to "unlabelled" before).

Frame 0's far bank, already non-trivial (not a placeholder run of zeros):

```
cell   0    1    2    3    4    5    6    7    8    9   10   11   12   13   14   15
type   0    1    2    1    2    2    1    2    1    1    2    1    2    2    1    2
hp     0    5    4    4    5    4    4  132    4    1    1    1    1    1    1    1
```

(cell 7's 132 = `0x84`, bit 7 set -- a bonus already placed on this cell
before the trace starts; see below.)

## Two bonus events, both banks in lockstep

```
frame 35   cell 7   far [2,132] -> [2,4]     near (grid) [2,132] -> [2,4]     bonus_6d9a 0 -> 2
frame 282  cell 8   far [1,4]   -> [1,132]   near (grid) [1,4]   -> [1,132]
```

Both are the placer/pickup mechanism already named in `docs/disc-notes.md`
("$9b28.../$9b32... SETS BIT 7, near bank ... and the far bank, same slot"):
`$9b28` and `$9b32` are two straight-line, unconditional writes to the same
slot in each bank, so a roll always marks both copies, and `$a29c andi.w
#$0f` (the near-bank pickup this project already models the writer of)
apparently strips both too -- frame 35's far-bank clear lands on the exact
same tick as the near bank's, not one tick later or never. This project's
prior citation ("this project's committed fixture only shows the near-bank
copy surviving to pickup" -- `bonus.provenance.md`) does not generalise to
every capture; this one shows both clearing together. Not a retraction of
that fixture's own finding (a slow diagnostic pass really did let its
far-bank copy get consumed first) -- just evidence that "surviving to
pickup" is not the far bank's fixed behaviour, only what one earlier,
slower capture happened to show.

## Gate: 34 ticks, both modes

```
$ cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/farbank.ndjson --skip-waived
DIVERGENCE at trace frame 35 ... tiles[7].hp expected 4 got 129
34 tick(s) matched before this one.

$ cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/farbank.ndjson
(identical -- player 2's rows do not diverge any earlier in this capture)
34 tick(s) matched before this one.
```

`disc-core` has no model for bit 7 on *either* bank (discr-dc0/discr-ovl.4's
gap: a plain `hp -= damage` subtraction reads 132 as a huge HP value and
computes `132 - 3 = 129`, oblivious to the flag), so it diverges the instant
the pickup strips it -- on the near bank's `tiles[7].hp` row, which schema
order checks before `tiles_far[7].hp`, so that is the row tracecheck names.
`tiles_far[7].hp` does not disagree any earlier: the seeded far-bank values
match the trace's own recorded ones for cells 0-15 on ticks 0-34 exactly,
which is the addressing/stride/seeding claim this fixture exists to make.
Gated in `mise.toml` at `FARBANK_MIN_AGREE = 34` (both bare and
`--skip-waived` give the same number here, so one gate line covers both).

## What this fixture does NOT reach: a genuine far-wall damage hit

`disc::step`'s far-wall branch (`$a5d6`/`$9f5e`, this bead's other half) only
calls `impact()` against `tiles_far` when a disc already owned by real
player 1 (raw owner `0xFF`, discr-ovl.8's polarity) crosses `world_z` = 79 a
SECOND time. Every wall crossing this fixture's own window contains is
either a disc's first arrival (the transfer arm, `$a5e2`/`$a624`, which
moves the four possession counters this project already doesn't model --
discr-st8 -- and never touches a tile) or happens beyond tick 34, where the
gate already stops. This capture's own disc 0 does reach `world_z` = 79 once,
at frame 198 (owner 0 -> 255, a transfer, `wz` 79) and returns to owner 0 at
frame 278 -- structurally identical to `handover.ndjson`'s frames 259/339,
just on a different seed -- and never comes back a second time before the
trace ends.

**What was tried, and why it stopped here**: getting a disc to hit the far
wall while ALREADY owned needs player 1 to reflect it (body-box bounce)
*before* it reaches the near wall's `world_z` = 0 -- reaching the near wall
transfers possession back unconditionally, closing the window. Three
approaches, all measured, none landing the hit within this oracle's usable
range:

1. **Idle, multiple fresh seeds** (this seed, plus two others minted the same
   way): every one cycles the SAME disc 0 -> 255 -> 0 (transfer, transfer
   back) with no interruption, because an idle player 1 is out of the disc's
   X range at every crossing.
2. **Scripted player 1 movement, timed to the return leg's known X
   trajectory**: landed player 1 within single digits of the disc's X at the
   crossing frame (one attempt: `p1x` = 18, `discx` = 18, same frame) with no
   resulting bounce -- `dir_kind` rode through unchanged, meaning either the
   body-box test wants more than X overlap at that sample, or the real
   collision resolves inside a multi-pass tick this single-sample trace
   cannot see (Part 11f/11g's "0, 1 or 2 passes a frame" applies here too).
3. **A closer approach overshot into the arena's left edge** (`world_x` = 8),
   which produced an unrelated player-1 state transition (a wall bump, not a
   disc hit -- `dir_kind` never changed) and, separately, a genuine KO from
   an EARLIER hit while owner was still 0 (the near bank's own force-and-
   damage arm, already modelled and already fixture-covered by
   `tile_damage.ndjson`), ending that round before a second far-wall
   crossing could occur at all.

All three runs hit the same hard ceiling regardless of input: the oracle
aborts around frame 460-500 on this seed shape with `UNSTUBBED write.w
$ff8606`/`$ff8604`/reads at `$fffa01`/`$ff8800` -- floppy disk controller and
PSG registers, consistent with a round-transition load the oracle's stub
list (built for in-round play, per `reports/oracle-scope.md`) was never
meant to cover. `--permissive` gets past the FIRST such access but not the
cascade that follows immediately after, so the practical window per seed is
under 460 frames regardless.

**Recipe for whoever picks this up**: script player 1 to arrive at the
disc's return-leg X **several frames before** the crossing tick (not
overshoot past it, which risks the arena-edge state above), and confirm the
hit with a `--dump`/multi-pass-aware capture rather than a single sampled
frame, so a hit that resolves mid-pass is not missed the way approach 2
above may have missed one. `scripts/collect.py`'s `-v` flag and a `--watch`
on the disc's own owner byte (not just the tile banks) would show the
attempt's outcome directly instead of inferring it from `dir_kind`.

## Files touched

* `tests/fixtures/farbank.ndjson` (new, `git add -f`) + this file.
* `crates/disc-tools/src/main.rs` -- `checks()`/`not_in_trace()` gain the
  `tiles_far[n].*` rows; `SCHEMA_COMPARED`/`SCHEMA_WAIVED` updated;
  `projection_is_never_compared`'s count formula extended.
* `docs/state-schema.md` -- two rows moved from Waived to Compared; prose
  updated in both places.
* `scripts/oracle_diff.py` -- `label_for` gains the `$7596..$7616` case.
* `oracle/disc-oracle.c` -- comment only, citing this bead against the
  already-existing `banks` column.
* `mise.toml` -- `FARBANK`/`FARBANK_MIN_AGREE`, wired into `core-check` and
  `tracecheck`.
* `crates/disc-core/src/disc.rs` -- the far-wall damage branch itself
  (shared with discr-ovl.8's commit; see `reports/part12-farbank.md`).

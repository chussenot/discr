# bonus_code1.ndjson -- provenance

bd discr-b6x + discr-z8m. 1100 frames, scripted. The first fixture to carry
the shared `$6c5d` PRNG stream (`rng_6c5d`, `latch_id`, `latch_prio`,
`gate_6e3c`, `mint_6e3a`, `code_6d9e`, `code_6d9c` -- all new
`oracle/disc-oracle.c` columns, `reports/part12-walls.md`), and the first to
catch bonus code 1 (the disc's damage applied twice, `$a314`/`$a31c`) live.

    ./oracle/disc-oracle --seed seeds/match_challenge.seed --frames 1100 \
        --permissive --script tmp/z8m_code1.script \
        --trace tests/fixtures/bonus_code1.ndjson

* **Seed**: `seeds/match_challenge.seed` (gitignored like every other seed),
  the same generic mid-CHALLENGE-round seed `farbank.provenance.md`/
  `handover.provenance.md` already used (minted via `scenarios/
  oracle_seed.yaml`, a live boot into a cached challenge match). THIS
  session's own capture: sha256 `bcfdb425fb460e0d12343ca31e8179c342206120b4b2c8cf563ad0dec0a47b47`,
  `$6ab4` = 6521 -- a different capture than farbank's own (`$6ab4` = 6312,
  different sha256), because the seed file is regenerated fresh by
  `scenarios/oracle_seed.yaml` each time it is minted and was not pinned
  between sessions. Reused rather than re-minted this phase: the recipe and
  cache (`tmp/match_challenge.sav`) are identical to three already-committed
  fixtures' own, so this is not a new emulation surface, only a longer
  script through the same one.
* **Input**: a scripted joystick programme, `tmp/z8m_code1.script` (523
  lines, kept local -- not committed, gitignored like every other `tmp/`
  script; reproducible from the recipe below). Derived from `disc-oracle
  --autopilot 11 4 10` (steer player 1 toward grid cell 11, fire every 4
  frames from frame 10) via `--emit-script`, then REPLAYED as a plain
  `--script` for the committed mint -- `--script tmp/z8m_code1.script`
  against the same seed reproduces this fixture byte-for-byte (checked:
  `diff` against the `--autopilot`-driven capture is empty). Not idle,
  unlike every fixture before it that needed a live rally: code 1 needs
  ~130 gate-fires to show up with reasonable odds (`P(code 1) = 2/128` per
  fire, `reports/part12-z8m.md`), and this project's own challenge rounds run
  out of match after a few hundred frames when idle (confirmed this phase:
  an idle run off the same seed stops advancing the bonus gate by frame
  ~300, well short of a code-1 roll in 18 independent tries by a prior
  phase's live-Hatari hunt) -- a sustained rally is what buys enough
  gate-fires to land one in a SINGLE deterministic run instead of 18 live
  boots.
* **Why the window is trustworthy**: NOT independently cross-validated
  against a live Hatari reference this phase (the 523-step scripted
  programme would need converting to wall-clock key events for
  `scripts/oracle_diff.py`'s `hatari_side()`, not attempted this session --
  same honest gap `p1_walk.provenance.md` already carries for its own input
  programme). What stands in its place: the finding this fixture exists to
  prove is verified from INSIDE the trace, not against a second emulator --
  three independent, undoubled `-3` hits (frames 107, 535, 656, each on this
  same seed's same character/disc combination) establish the baseline damage
  constant directly from the trace's own recorded HP deltas, and the two
  code-1 hits (frames 992, 999) are checked against that SAME baseline, not
  an assumed one. See "The code-1 double-apply" below.
* **`--permissive`**: the scripted rally outlives the challenge round in
  this trace (`tmp/hunt2.err`-equivalent: unstubbed reads/writes to the
  sound chip, `$ff8604`/`$ff8606`/`$ff8802`, once the round ends and a
  menu/results screen is reached) -- forgiven rather than fatal, since
  nothing this fixture is cited for depends on sound hardware state. The
  round itself (live rally) plays out over roughly the first 1000 frames;
  what follows is idle noise the trace is simply not read past.

## The RNG stream (discr-b6x)

`oracle/disc-oracle.c` now samples `$6c5d` (`rng_6c5d`), the AI dispatch's
own latch (`$6da6`/`$6daa` -> `latch_id`/`latch_prio`), and the bonus mint's
gate/payload (`gate_6e3c`, `mint_6e3a`, `code_6d9e`, `code_6d9c`) every
frame -- ground truth, not modelled. `crates/disc-core/src/ai.rs`'s
`rng_verify::bonus_code1_stream_within_bound` replays 891 of this fixture's
1099 consecutive frame-pairs (208 skipped: 138 multi-pass, 70 where the
sampled latch itself changed -- both cases `rng::eligible_rolls_bound`'s own
doc names as not exact) and checks the observed `$6c5d` delta is reachable
within the KNOWN dispatch loop's own roll budget for that pass's latch
state, plus 2 more on the 29 frames where `gate_6e3c` shows the bonus mint
also fired. **All 891 pass** -- zero unaccounted deltas. Two shapes worth
citing directly:

* **64 pairs need exactly 0 rolls** -- every one of them a frame where the
  latch priority is already 50 (row 0, the escape, active), which by
  `rng::eligible_rolls_bound`'s own table admits no other row at all. `$6c5d`
  provably does not move on these frames, and it doesn't.
* **3 pairs need the full 20** -- frames where nothing was latched
  (`latch_id == 0`), matching `reports/part12-rng.md`'s own live-Hatari
  finding, independently, in a deterministic oracle run: "with nothing
  latched... a single pass can burn on the order of twenty rolls at once."

## The code-1 double-apply (discr-z8m)

| frame | cell (near grid) | `bonus_6d9a` | hp before -> after | reading |
|---|---|---|---|---|
| 107 | 6 | 5 (unrelated active code) | 4 -> 1 | undoubled: -3 |
| 535 | 8 | 5 | 4 -> 1 | undoubled: -3 |
| 656 | 1 | 5 | 5 -> 2 | undoubled: -3 |
| 644 | -- | mint: `$6e3a` becomes 1 | -- | code 1 minted (`$9d52`/`$9d58`) |
| 776 | 7 | 5 -> 1 | 132 -> 4 | PICKUP (bit 7 stripped, no damage) -- `$a292`-`$a2ca`, walk/strike-over a FLAGGED cell does not damage it |
| 992 | 5 | 1 (`code_6d9e`: 5 -> 4) | 4 -> 0 | **DOUBLED: -6, clamped to 0** |
| 999 | 2 | 1 (`code_6d9e`: 4 -> 3) | 4 -> 0 | **DOUBLED: -6, clamped to 0** |

The disc's own damage constant for this match/character is established
directly from the fixture, not assumed: three hits (107/535/656), all with
`bonus_6d9a` at an UNRELATED active code (5, which per `docs/disc-notes.md`'s
`$9aa2` table gates the catch-reach mechanic, not damage), each move a
cell's hp by exactly -3. A single such hit on an hp-4 cell (107, 535) always
leaves hp 1 -- it cannot reach 0. At frames 992 and 999, an hp-4 cell is
instead destroyed OUTRIGHT by one strike, both while `bonus_6d9a == 1` (the
code minted at frame 644, picked up frame 776) is the active, not-yet-spent
effect (`code_6d9e`, the code's own consumable count from the `$9aa2` table,
visibly decrements 5 -> 4 -> 3 on exactly those two frames -- the same
signal `reports/part12-z8m.md` read for code 3). The only damage value
consistent with `hp 4 -> 0` in one hit, given the established -3 baseline,
is -6: the disc's damage applied twice, exactly `$a314`/`$a31c`'s own
decoded semantics (three prior phases left unexercised).
`crates/disc-core/src/tile.rs`'s `bonus_damage_multiplier` and its
`tile_bonus_code1::replays_every_hit_frame_exact` test reproduce all five
rows above frame-exact through the existing `damage()` function.

## tracecheck (informational; not gated by `mise run core-check`)

This fixture drives player 1 through a rally shape (`--autopilot 11 4 10`)
none of the other six committed fixtures use, and surfaces two ALREADY-KNOWN
gaps earlier than they otherwise would: `players[0].anim_cursor` under
`--skip-waived` (27 ticks; a pre-existing animation-table gap, not new --
`reports/part12-anim.md`'s own player-1 animation model does not yet cover
every input transition this script exercises) and player 2's own unhandled
`$c6ec` states without `--skip-waived` (17 ticks; the same discr-b6x
limitation every other fixture already waives around). Recorded honestly
(`BONUS_CODE1_MIN_AGREE`/`BONUS_CODE1_FULL_MIN_AGREE` in `mise.toml`) but
not added to `core-check`'s fixed ten-invocation gate list -- this fixture's
job is the RNG stream and the code-1 measurement above, not a new
simulation-fidelity high-water mark.

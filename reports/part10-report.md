# Part 10 — decoding player 2, and cashing in the waivers

The phase's brief was to decode the opponent, close the mechanical leftovers,
then un-waive rows and raise the tracecheck gate. Ghidra 12.1.3 arrived with
it, and that changed the shape of the work: five of the six unknowns this phase
was meant to grind out were answered by *reading the code* in the first hour,
and the rest of the time went on turning those reads into measured agreement.

Same standard as the reports beside this one. Every number was produced by a
command in this file, and every gap is named with the bead that owns it.

    scripts/ghidra/import.sh                 # once, ~90 s
    scripts/ghidra/q.sh dis a4ea 60
    mise run core-check                      # the two gates
    mise run tracecheck-deep                 # the ungated measurement

## The scoreboard, before and after

`mise run core-check`'s gate is the length of the matching prefix. There are now
two, because the two fixtures hit different walls.

| run | before Part 10 | after | the wall it stops on |
|---|---|---|---|
| `golden.ndjson --skip-waived` | **10** | **10** | `players[0].state_index` — discr-75o, the player state machine, untouched this phase |
| `tile_damage.ndjson --skip-waived` | **10** | **51** | the ST re-serves disc 0 at frame 52, from inside player 2's control routine — discr-b6x |
| `golden.ndjson --skip-waived --resync players[0]` | **22** | **51** | the same re-serve |
| `tile_damage.ndjson --skip-waived --resync discs[0]` | 69 | **118** | `tiles[14]` at frame 119 — discr-b4q's anomaly |

Read the second row as the headline: **on the idle fixture, with nothing
resynced at all, `disc-core` now reproduces 51 consecutive ST frames.** It used
to reproduce 10. The idle fixture is the one that measures the disc model,
because an idle player 1 never changes state and so never trips discr-75o.

Read the third row as the like-for-like comparison: same fixture, same resync
set, **22 → 51**, and the two rows that used to have to be supplied to get past
frame 22 — `discs[0].world_y` and the steering — are now produced.

Read the fourth as the one that matters for the tile module: it runs *past* the
tile event at frame 70, which `disc::step` now causes itself. **`tile::damage`
is exercised end to end by trace comparison for the first time**, not only by
unit tests. It is deliberately not gated: a resynced row is one `disc-core` was
given, not one it produced.

`mise run core-check` gates rows 1 and 2 at `TRACE_MIN_AGREE` = 10 and
`TILE_MIN_AGREE` = 51. Row 4 is `mise run tracecheck-deep`, ungated.

## What was decoded, and how much of it a trace confirms

Everything below is in `docs/disc-notes.md` with its instructions. The column
that matters is the last one.

| finding | ST | confirmed by |
|---|---|---|
| One-player mode is selected by `$6da0`; `$d2cc` writes a synthetic joystick byte to `$6da1`, consumed by player 2's control routine `$abb2` exactly where a human's `$6c59` goes | `$10eac` | **trace** — `$6c59` is 0 on all 240 frames, `$6da1` takes ten joystick bit patterns |
| The opponent is a 20-entry priority rule table at `$efa8`: priority, a reaction threshold out of 255, and test/action/identity routine pointers, gated by a one-byte PRNG `$6c5d += $6ab5` | `$d2cc` | **trace** (that it runs); the individual rules are code only |
| `disc+$10` bit 7 is the active flag; `disc+$11` the owner; `disc+$12` a per-disc steering hook | `$a4f0`, `$a534`, `$a55e`, `$a54c` | **trace** — `act` reads `$ff`/2/1/0, `hook` reads `0`/`$a7d8`/`$a816` |
| `world_y += vel_y`, and `vel_y` decays toward zero *after* the integration | `$a556`, `$a640` | **trace** — golden frames 23-24 |
| The steering "gate" is the hook. Player 1's hit test installs `$a71a`; player 2's installs `$a7d8` (X only) or `$a816` (both axes) | `$113e2`, `$cb70`, `$cbae`, `$cc1e` | **trace** — golden frames 11-28 show `$a7d8` holding `world_y` at 81 and `$a816` taking it to 83 |
| The collision test is `world_z` crossing a wall; the struck cell is `column(world_x + 4) + (4 if world_y > 70)`, landing in 1..8 | `$a5fe`, `$a618`, `$a24c` | **trace** — the frame-70 event now reproduces |
| `world_x` is bounded 0..155 and `world_z` 0..79, each bound negating its velocity | `$a58e`, `$a5a6`, `$a5ba`, `$a5fe` | `world_x` **trace** (155 is reached exactly); `world_z` = 79 is **code only** |
| `$6d9a` is the active bonus code, 1..5, picked up from a cell whose HP word has bit 7 set, with payload and duration from the table at `$9aa2` | `$824c`, `$a292`, `$a2b0` | **code only** — `bonus_6d9a` is 0 on every frame of both fixtures |
| The far wall has a **second** tile grid at `$7596` with its own bonus word | `$9f5e` | **code only** |
| A serve fires when player 2's animation cursor `$6d5a` reaches `$4602` | `$c06e`, `$c0c0` | **code only** — the trigger is in code `disc-core` does not model |

## The `$6d9a` rank hypothesis is refuted

The brief named this as the cheap high-leverage test: patch `$6d9a` to 1, 2 and
3 in a seed copy and compare three oracle runs. **That experiment was not run,
because reading the code settled it more strongly than three runs could have.**

    $8240  tst.w $6d9c ; beq $8250
    $8246  subq.w #1,$6d9c            ; a countdown, every VBL
    $824a  bne $8250
    $824c  clr.w $6d9a                ; timer expired -> the code goes away

A per-match difficulty rank is not decremented to zero by the VBL handler. What
`$6d9a` is instead: the **active bonus code**, set at `$a2b0` from `$6e3a` when a
disc strikes a cell whose HP word has bit 7 set, with its payload in `$6d9e` and
its lifetime in `$6d9c`, both read from the four-byte-per-entry table at `$9aa2`:

| code | `$6d9e` | `$6d9c` | what it gates |
|---|---|---|---|
| 1 | 5 | none | `$a314`: a second application of the disc's `+$16` damage |
| 2 | 0 | 500 frames | `$c09c`: serve with `dir_kind` −5 instead of `$6d8e` |
| 3 | 3 | none | `$a32e`, and three sites inside the AI |
| 4 | 0 | 1000 frames | `$c9b4`/`$c9c8` in player 2's hit test |
| 5 | 0 | 1000 frames | `$cb56`/`$cb78`: catch reach becomes a flat 50 |

Three tests against 1, 2 and 3 looked like three ranks. They are three of five
bonuses. Running the patch experiment would have produced three differing runs
and *supported* the wrong conclusion, because patching a bonus code does change
damage magnitudes and serve behaviour — which is exactly what the experiment was
going to look for. Worth recording as a near miss: the experiment was well
designed for a hypothesis that a single instruction disproves.

The interview's three claims are not resolved by this and are now three separate
leads, not one: tiles taking more hits is bonus codes 1 and 3; more discs is
`$6d8a`/`$6d8c`; a tougher AI would be a different rule table at `$6da2`, whose
writer is not in the analysed image.

## The retraction

`docs/disc-notes.md` Part 9 said: *"Do not model vertical motion as `world_y +=
vel_y`… A core that integrates `world_y` by `vel_y` will overshoot the aim point
and oscillate."* `docs/state-schema.md` repeated it. Both are wrong, and
`$a556 add.w ($08,a5),d1` is unconditional.

The observation behind the claim was right — `vel_y` is 0 at every sampling
point in every trace. The inference was wrong because **`$a640` decays `vel_y`
toward zero after the integration and before the sampling point**, so a
one-frame impulse moves `world_y` by one and reads back as zero. It is
structurally invisible.

It also does not oscillate, for a reason the trace shows: under the `$a816` hook
the aim is 83, and `world_y` climbs 81 → 82 → 83 on golden frames 23-24 and then
stops dead, because at the target the rule decays instead of pushing. The
predicted behaviour and the observed behaviour are the same three frames.

This is the second time a "do not model X" note in this project turned out to be
an over-reading of a correct measurement — the first was discr-217, "the
steering block is gated off". Both had the same shape: a rule was tested against
one hypothesis about its inputs, the hypothesis was wrong, and the conclusion
was written about the *code* rather than about the hypothesis.

## The waiver accounting, honestly

`docs/state-schema.md` has **12 waived rows, the same number as before**. The
brief wanted the list visibly shorter; it is not, and the reason is worth
stating plainly rather than shuffling rows to make a number move.

**Five waivers came off**, all resolved by reading rather than by modelling
harder: discr-217 (the steering gate), discr-tan (`world_y`'s writer), discr-5w5
(the collision test), discr-dc0 (the tile bit-7 writer) and discr-m4x (the serve
trigger). Two more closed alongside them: discr-1q7 (the `world_x` ceiling, 155)
and discr-fnl (the dwell/state-17 correlation, which was the serve).

**Five new ones went on**, every one of them exposed by an answer:

* **discr-ovl.1** — what *installs* a hook. Answering "what gates the steering"
  turned into "which of three routines does a hit test choose".
* **discr-ovl.2** — `disc+$11`'s polarity. The owner byte is real; no trace has
  ever seen it non-zero, so which value means which player is a guess.
* **discr-ovl.3** — the second tile grid at `$7596`, which nobody knew existed.
* **discr-ovl.4** — what *places* a bonus. Resolving the pickup exposed the
  placement.
* **discr-qqt**, reopened — `$6d8e` is `player+$6e`, a per-player throw
  direction-and-speed (p1 reads +1, p2 reads −3, magnitudes repeated at `+$70`).
  It was closed in the sweep by mistake and reopened, because knowing *what a
  field is* is not knowing *what writes it*.

So the count is flat and the position is not. Before this phase, six undecoded
rules stood between `disc-core` and the fixture — the steering gate, `world_y`'s
writer, the collision test, the disc bounds, the active encoding and the aim
selector. Now one does: the serve, at frame 52. That is what the 10 → 51 in the
scoreboard is measuring.

## What Part 10 did not do

Stated rather than glossed, in the order a next phase should probably take them.

* **The AI's rules are not decoded, only its architecture.** The table at
  `$efa8` has 20 entries and there are 11 distinct test routines and 7 action
  routines behind them, plus a sensor pass at `$cea6` and whatever `$6dac` and
  `$6dfc` carry. `disc-core` has no `Controller`, and the brief's deliverable —
  a Rust controller matching observed `$6da1` traces divergence-first — was not
  built. What exists instead is the trace column (`ai_6da1`) that such a
  comparison would need, and the certainty that `$6da1` is the whole channel.
* **The player state machine is untouched**, so the `golden.ndjson` gate is
  still 10 and discr-75o and discr-xfw are exactly where they were. This is the
  single biggest remaining blocker and it is the obvious next headline.
* **No bonus is exercised by any trace.** Every row of the `$9aa2` table is a
  code read. A trace in which a disc strikes a bit-7 cell is now the
  highest-value fixture this project does not have — it would test five effects
  at once.
* **The far grid at `$7596` has never been compared**, by `disc-core` or by
  `scripts/oracle_diff.py`. The differ's memory window already covers it.
* **Two fed inputs.** `tracecheck` supplies `disc+$12` and `disc+$10` bit 7
  every tick, the way it supplies `$6c58`, and prints a header line saying so.
  Both are written by code outside `$a4ea` (discr-ovl.1, discr-0fm). A number
  produced with a fed input is not a number `disc-core` produced alone, and the
  tool says which is which on every run.
* **`world_z`'s far bound of 79 is a code read with no trace behind it.** Every
  disc we have recorded is returned or retired by about 54.

## Method notes worth keeping

* **`scan` matters as much as `xref`.** Ghidra's reference model files
  `move.l #$a7d8,($12,a5)` as an immediate, not a reference, so the three
  steering-hook install sites are invisible to `xref` and obvious to a linear
  operand scan. The whole of discr-217 turned on that one query.
* **Seeding entry points is what makes a raw RAM image analysable.** A 1 MB
  image has no symbols and no entry point, so auto-analysis disassembles
  nothing. `SeedEntries.java` disassembles the 70 addresses
  `docs/disc-notes.md` had already earned, and analysis then reaches the whole
  game loop by following calls. The knowledge from nine phases of black-box work
  is the input that makes the static tool work at all.
* **Regenerating a fixture is safe when the change is additive.** Both fixtures
  were re-emitted with six new per-disc columns and four per-frame ones, and
  every pre-existing column is byte-identical on every frame — checked, not
  assumed. Pre-Part-10 traces still load, because the new columns default.
* **A "do not model X" note is a claim about code and deserves the same
  evidence.** Two of them have now been retracted. Both were correct
  observations paired with an untested assumption about the rule's inputs.

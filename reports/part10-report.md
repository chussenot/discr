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

| run | before Part 10 | after 10 | after 10b | the wall it stops on |
|---|---|---|---|---|
| `golden.ndjson --skip-waived` | **10** | 10 | **51** | the ST re-serves disc 0 at frame 52, from inside player 2's control routine — discr-b6x |
| `tile_damage.ndjson --skip-waived` | **10** | 51 | **51** | the same re-serve |
| `golden.ndjson --skip-waived --resync players[0]` | 22 | 51 | — | superseded: the player rows no longer need supplying |
| `golden.ndjson --skip-waived --resync discs` | — | — | **63** | `players[0].world_y` at frame 64, where player 1's hit test `$10fd8` enters state 11 — discr-ovl.1 |
| `tile_damage.ndjson --skip-waived --resync discs[0]` | 69 | **118** | 118 | `tiles[14]` at frame 119 — discr-b4q's anomaly |

Read the first row as the headline: **on both fixtures, with nothing resynced
but player 2, `disc-core` now reproduces 51 consecutive ST frames.** It used to
reproduce 10. Part 10 got the idle fixture there by fixing the disc loop; Part
10b got the *walking* fixture there by modelling the player state machine, so
both now stop at the same wall — the serve, which is player 2's action and the
one thing on either side that is still missing.

Read the fourth and fifth rows as what each half reaches when the other is
supplied. The player half runs to 63 and stops where player 1's hit test moves
its `world_y`; the disc half runs to 118, *past* the tile event at frame 70,
which `disc::step` now causes itself. **`tile::damage` is exercised end to end
by trace comparison for the first time**, not only by unit tests. Neither is
gated: a resynced row is one `disc-core` was given, not one it produced.

`mise run core-check` gates rows 1 and 2, both at 51. Rows 4 and 5 are
`mise run tracecheck-deep`, ungated.

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

* **The AI's mechanism and its two reflexes are decoded; its other 18 rules are
  not.** What `$6dac`/`$6dfc` carry is now known — a rule's action compiles a
  two-parameter *plan* into that buffer and its identity routine executes the
  plan once a frame and decides when the behaviour is over, which is why
  priority behaves as a latch. Entry 0 (priority 50, unconditional) fires when
  the floor cell player 2 is standing on has reached zero hp and reads an escape
  table at `$1556`/`$155e`; entry 1 (priority 30, unconditional) is a
  three-dimensional box test around player 2 for a live disc, with every
  threshold read from player 2's own record rather than from a literal. The
  other 11 tests, 7 actions and the sensor pass `$cea6` are undecoded, and
  `disc-core` has no `Controller` — so the brief's deliverable, a Rust
  controller matching observed `$6da1` traces divergence-first, was **not
  built**. What exists towards it is the trace column (`ai_6da1`), the certainty
  that `$6da1` is the whole channel, and the mechanism a controller would have
  to implement.
* ~~**The player state machine is untouched.**~~ Done in Part 10b — see below.
  What is left of discr-75o is the other 28 handlers' behaviour and the
  animation frame data their sequences point at.
* **No bonus is exercised by any trace.** Every row of the `$9aa2` table is a
  code read. A trace in which a disc strikes a bit-7 cell is now the
  highest-value fixture this project does not have — it would test five effects
  at once.
* **The far grid at `$7596` has never been compared**, by `disc-core` or by
  `scripts/oracle_diff.py`. The differ's memory window already covers it. And
  the discovery that a bank is **16** cells means `TILE_CELLS = 17` compares one
  word that is not a tile — filed as discr-ovl.5, not fixed inline, because it
  moves the schema counts, the fixture column and the differ together. No result
  depends on it: every tile event ever observed sits inside the bank.
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

---

# Part 10b — the player state machine

The "obvious next headline" above, done. One more Ghidra session, and the
`golden.ndjson` gate moved **10 → 51**, which is the number the whole phase was
trying to move.

## What it turned out to be

Not 32 unrelated handlers. **One mechanism.** `$f104`'s first two instructions
are `tst.b $6cae; bne $f5d0` — so state 0 is not a table entry at all (entry 0
of `$10e2c` is a null longword), it is the code that follows. And every handler,
including that one, ends in the same tail at `$f1c4`:

* an animation sequence is a list of **six-byte cells** — a four-byte frame
  pointer and a two-byte hold count — ending in a zero longword;
* `$6ce2` counts the current cell's hold down once per frame, `$6cda` walks the
  cursor forward six bytes when it expires;
* **running off the end of the sequence is what changes state.** `$f1c4`'s
  ending goes to state 0; state 20's copy of the tail (`$1099a`) goes to
  `$6caa`, the pending state.

That makes the transient reproducible to the frame rather than fitted. Pressing
Left from idle writes `$6caa = 1`, loads the sequence at `$2f7e` — **one cell,
hold 4, then the terminator** — sets the state to 20 and falls into the tail in
the same tick, so the count reads 3 at the first sample. Three more handler runs
and the state becomes 1. The fixture shows exactly three frames of state 20, at
f11-f13 and again at f29-f31.

`$6cda` and `$6ce2` were `excluded:rendering` in `docs/state-schema.md`. They
are the state machine's clock. That row is `waived:discr-75o` now, and the
excluded count drops 6 → 5 while the waived count rises 12 → 13.

## discr-xfw answered: `$6ca9` is not a facing flag

Every handler's *first* instruction stamps its own state number there — `$f5e2`
writes 1, `$f7f6` writes 2, `$1094a` writes `$14`, `$109aa` writes `$15`, and
the idle path clears it at `$f1c0`. A handler may then change `$6cae` before the
frame ends, so the sampled `$6ca9` is **the state whose handler ran this frame**
while `$6cae` is the state that will run next. That is the one-frame lag, and it
explains the three exceptions the old analysis could not place: those are frames
where a handler ran and the state changed twice.

`disc-core` writes it from the handler now, so the row passes for the right
reason rather than by coincidence on states 1 and 2.

## What is modelled, and what is not

States **0, 1, 2 and 20** — every state player 1 reaches in either fixture
before its hit test fires. The other 28 entries stay opaque pass-throughs: their
handler addresses are known, their behaviour is not, and their sequences point
at frame-block data this crate does not carry. Entering one would mean running a
state `disc-core` cannot simulate.

Two smaller facts fell out:

* **`$6cba` is a per-frame X delta lifted out of the animation frame block**,
  applied by the idle path at `$f118`. Some movement is authored in animation
  data rather than in code; the walk states do their own `subq.w #3` on top.
* **Player 2 has its own 32-entry table at `$c6ec`**, not `$10e2c`. Entry 15 is
  `$c068` — the block containing the serve — so the serve is player 2's *state
  15*, and `$c0c4` moves it to 17 on the frame the disc leaves.

## The wall, and it is the same one on both sides

Both fixtures now stop at trace frame 52, `discs[0].world_x` 45 vs 48: the ST
re-serves disc 0 into the same slot and `disc-core` has no trigger for it. The
trigger is known — player 2's animation cursor `$6d5a` reaching `$4602` — but it
lives inside player 2's control routine, which means modelling player 2's state
machine and the AI that drives it. **discr-b6x is now the single thing standing
between `disc-core` and the rest of both fixtures**, and the machinery to do it
exists: `$c6ec` is the table, `$abb2` the dispatcher, `$d2cc` the policy, and
`ai_6da1` is already a trace column.

---

# Part 10c — the serve, and player 2 stops being unmatchable

The wall Part 10b left is gone. The serve is implemented from its two decoded
throw states, and player 2 — whose five rows had never matched a single frame in
the project's history — now matches for 21 ticks with nothing waived at all.

## The scoreboard, three gates now

| run | what it measures | before Part 10 | after 10 | after 10b | **after 10c** |
|---|---|---|---|---|---|
| `golden --skip-waived` | everything but player 2 | 10 | 10 | 51 | **63** |
| `tile_damage --skip-waived` | the same, on the idle fixture | 10 | 51 | 51 | **118** |
| `golden` (no flags) | **nothing waived, nothing resynced** | 0 | 0 | 0 | **21** |

The walls, in the same order:

* **63** — `players[0].world_y` at frame 64, where player 1's hit test `$10fd8`
  puts it into state 11 and moves it. discr-ovl.1's other half.
* **118** — `tiles[14]` at frame 119, the anomaly discr-b4q owns: a cell's type
  cleared with its hp intact. This run now goes *past* the frame-70 tile impact
  with **no `--resync` at all**, where Part 10 needed `--resync discs[0]` to
  reach the same number.
* **21** — player 2 enters state 18 at frame 22, one of the 28 handlers of its
  own table at `$c6ec` that `disc-core` has no code for.

The third row is the one worth keeping an eye on. It is the strictest thing this
repo measures: all 15 compared rows of *both* players, nothing supplied but the
two joystick bytes and the four fed ST inputs. It used to stop on frame 1 by
construction.

## What made the serve implementable

Three things, in the order they mattered.

**The gate is `player+$3a`, and it is the animation cursor Part 10b already
modelled.** `$c06e cmpi.l #$4602,$6d5a` is not a timer and not a disc test: it
fires on the single frame of the throw animation where the sequence cursor
reaches one exact value. `$6d5a` is player 2's `+$3a`, the same field as player
1's `$6cda`. The oracle emits it now, and the fixtures show the release frame
plainly — `$45fc, $45fc, $4602` and then state 17.

**There are two throw states, not one.** `$c6ec` entry 16 is `$c0fe`, which is
`$c068` with two constants swapped: the gate is `$45da` and `world_x` is
`p2.x + 3` rather than `p2.x - 9`. Golden's second serve, at frame 76, is that
one — player 2 at x 49 puts the disc at 52, and the state-15 offset would have
given 40. Six more `bsr $a972` sites exist inside `$abb2`, so there are further
throw states nobody has read.

**The slot fill did not stop at `$a9b4`.** The four instructions after it are
`st ($10,a1)`, `clr.b ($11,a1)`, `move.l a2,($12,a1)` and — the one that
mattered — `move.w $6d90,($16,a1)`. **A disc's damage is the thrower's
`player+$70`**, 3 for player 2 and 1 for player 1, the same magnitudes as their
`+$6e` dir_kinds. So a player's throw carries one number that is at once its
depth speed and its damage. Where `$a9a0` got the damage from had been open
since Part 8.

## A retraction, and a timing measurement

**Retracted:** Part 10 recorded that `$c0d0`/`$c0e8` "double the `vel_x`
adjustment" when `d2` is -1. The branch is a `beq` *skip*: a `dir_kind` of -1
gets the single sideways step and everything else gets two. Golden frame 52
settles it — a -3 disc served with right held reads `vel_x` +2, which the
old reading cannot produce. Third retraction of the project, and the same shape
as the other two: a correct reading of an instruction, paired with a guess about
which way the branch fell.

**Measured:** `$6da1` is written and consumed inside one VBL — `$10ec6 bsr
$d2cc` writes it, `$10ece bsr $abb2` consumes it two instructions later — so the
byte a frame's work uses only becomes visible at the *next* sampling point.
`$6c58` is different: the IKBD handler writes it asynchronously, so the sample
at frame N is already the byte frame N will consume. `tracecheck` therefore
drives player 2 from the frame it is predicting and player 1 from the frame it
starts at, which looks wrong until you see why.

It was found the hard way: feeding player 2 the current frame's byte took the
idle fixture *backwards*, 118 → 51, on a `vel_y` of -5 the ST never served. At
`tile_damage` frame 51 the sampled byte is `$81` (fire + up) and the disc served
on frame 52 has `vel_y` 0; frame 52's byte is `$80`, which serves 0.

## Honest limits on the new numbers

* **Four ST fields are fed every tick**, and the header line says so on every
  run: `disc+$12` (the steering hook), `disc+$10` bit 7 (whether the ST
  simulates a record), and `player+$3a`/`+$6e`/`+$70` (the animation cursor the
  serve gates on, and the two throw parameters). Every one of them is written by
  code outside the loops `disc-core` models. **The serve trigger itself is not
  synthesised** — it is the ST's own comparison against a fed cursor value — but
  a number produced with a fed input is not a number `disc-core` produced alone.
* **`player+$6e` and `+$70` have no writer anywhere in the analysed image**, so
  they are fed rather than derived (discr-qqt). They are constant in every trace.
* **The `$6d9a == 2` serve variant is not modelled.** No trace has carried a
  non-zero bonus code, so there is nothing to test it against.
* **`players[1].*` is still waived**, and the reason changed rather than went
  away: it is no longer "the trace carries no input for player 2" but "player 2
  reaches 28 states this crate has no handler for". The waiver list grew 13 → 15
  for the two new fed-input rows.

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
  puts it into state 11 and moves it. Taken down in Part 10d below.
* **118** — `tiles[14]` at frame 119, the anomaly discr-b4q owns. Closed in
  Part 10e below.
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

---

# Part 10d — the hit test, and a fixture reproduced end to end

`tests/fixtures/golden.ndjson` now runs **99 ticks with no divergence at all**.
That is the first time this project has reproduced a whole trace.

## The scoreboard

| run | what it measures | pre-10 | 10 | 10b | 10c | **10d** |
|---|---|---|---|---|---|---|
| `golden --skip-waived` | everything but player 2 | 10 | 10 | 51 | 63 | **99 — the whole fixture, clean** |
| `tile_damage --skip-waived` | the same, idle fixture | 10 | 51 | 51 | 118 | 118 |
| `golden` (no flags) | nothing waived, nothing resynced | 0 | 0 | 0 | 21 | 21 |

The first row is no longer a prefix length. It is the entire trace: player 1's
walk, both turn transients, **both strikes**, its energy going 5 → 2 → 0, the
death sequence, and the disc's whole flight including two serves, a floor bounce
and two returns. `mise run core-check` still gates on the prefix length rather
than on cleanliness, because that is what catches a regression — the number can
only go down from here.

The other two are unchanged and their walls are unchanged: the frame-119 tile
anomaly (discr-b4q) and player 2's state 18 (discr-b6x).

## What `$10fd8` turned out to be

Called from the disc loop at `$a652`, **between the integration and the
write-back** — which is the detail that makes it work. It gets the three
candidate coordinates in registers and can put the disc back where it struck.

Three findings worth separating out:

**The hit box comes out of the animation.** `$6cbc`, `$6cbe`, `$6cc0` and
`$6cc2` are four of the words `$f1ca` copies out of the current animation cell's
frame block every frame, so the box changes shape as the sprite does — player 1
reads `[-3, 11, -20, 18]` standing and `[-4, 11, -19, 16]` on the first frame of
being knocked down. `$6ca4`, the constant 99 Part 9 identified as a "height
reference", is the origin the vertical half is measured from. `disc-core` does
not carry the frame blocks, so the box is a fed input.

**`player+$76` is energy and `player+$0c` is "out".** `$11178` subtracts the
striking disc's `+$16` — which Part 10c had just established comes from the
thrower's `+$70` — clamps at 0, and sets `$6cac`. Player 1's 5 → 2 → 0 across
the fixture is now a *compared* row, the 16th.

**A knocked-down player sinks one row per animation cell, not per frame.**
State 11 opens with `cmp.l $6ce4,d0; beq` — the block copied last frame against
the cell about to show — so the vertical movement is paced by the sequence
advancing. `$2d50` is two cells of four, which is exactly the 18, 17, 17, 17,
17, 16, 16, 16 the fixture reads across frames 63–70.

That comparison also justified generalising the animation engine: `AnimSeq`,
`anim_cell`, `anim_shown` and `anim_tick` are now a faithful `$f1c4`, and the
turn transient's hand-rolled countdown was rewritten in terms of them. State 23
is a *variant* of the same tail — it tests for the terminator before copying and
does not change state on reaching it, which is what makes being out of energy
terminal rather than another transient.

## Two orderings, both measured rather than assumed

**The disc loop runs before the player update.** Part 10c had already shown the
serve must come after the disc loop (a disc served on frame N is not integrated
on frame N). Part 10d needs the strike to come *before* the player update, so
that state 11's handler runs in the same tick and `world_y` moves on frame 64
rather than 65. Both are satisfied by the ST's actual order, and switching
`tick` to it changed none of the three numbers — which is the check that it was
safe.

**`$6da1` is written and consumed inside one VBL** (Part 10c), so player 2 is
driven from the frame being predicted while player 1 is driven from the frame
the tick starts at. Recorded here too because it is the single most
counter-intuitive line in `tracecheck`.

## Honest limits

* **Six ST fields are fed every tick** now, up from four: `disc+$12`,
  `disc+$10` bit 7, and `player+$3a`/`+$1c..$22`/`+$6e`/`+$70`. Every one is
  written by code outside the loops `disc-core` models, the header line names
  them all, and none of them is a decision — they are sprite data, an animation
  cursor and two constants.
* **The racket path is not modelled.** States 7..10 of `$10fd8` catch the disc
  in a second, wider box from `$6cc6`/`$6cc8`, add `$6cc4` to its `vel_x`, and
  install the `$a71a` steering hook at `$113e2`. That is the last thing between
  `disc-core` and not being fed `disc+$12`, and player 2's `$c826` is its twin.
  Neither fixture reaches it: player 1 never swings.
* **The two bonus branches in the energy path are not modelled** — code 4 is a
  shield (`$1117c`) and code 1 doubles the damage (`$11188`). No trace carries a
  bonus code.
* **99 ticks is one fixture.** The idle fixture still stops at 118 and the
  fully-compared run at 21. A clean run on `golden` means `disc-core` is right
  about everything that fixture does, not about everything the game does.
* **`player+$0c` has no trace column**, so the "out" flag is produced and never
  checked. It is only reachable after the energy row, which *is* checked, hits
  zero — but that is an argument, not a measurement.

---

# Part 10e — both fixtures clean

`tile_damage.ndjson` now runs **214 ticks with no divergence**, alongside
`golden.ndjson`'s 99. Both traces this project has, reproduced end to end.

## The scoreboard

| run | pre-10 | 10 | 10b | 10c | 10d | **10e** |
|---|---|---|---|---|---|---|
| `golden --skip-waived` | 10 | 10 | 51 | 63 | 99 clean | **99 clean** |
| `tile_damage --skip-waived` | 10 | 51 | 51 | 118 | 118 | **214 clean** |
| `golden` (no flags) | 0 | 0 | 0 | 21 | 21 | 21 |

Getting there took three findings and one new tool.

## The tool: `--watch` on the oracle

`--watch LO HI` reports every write into an address range with the PC that made
it and the frame it happened on. "Who writes this address" is the question this
project asks most often, and answering it in Hatari costs a debugger session per
attempt. One oracle run answers it for a whole range at once.

The census that closed discr-b4q, five writes across both tile banks in 215
frames:

```
frame  69  pc $00a34c  write.w $007648 = $0000     cell 6 hp
frame  69  pc $00a354  write.w $007646 = $0000     cell 6 type
frame 118  pc $014bb8  write.w $007686 = $0000     cell 14 type   <-- the anomaly
frame 169  pc $00a34c  write.w $007650 = $0001     cell 7 hp
frame 207  pc $00a34c  write.w $007658 = $0001     cell 8 hp
```

`--disasm ADDR N` came with it, running Musashi's own disassembler over the
seeded RAM. `$14bb8` sits in a region the Ghidra import never reached — a raw
1 MB image is mostly unreachable from any seed — and re-importing to read twenty
instructions costs ninety seconds.

## discr-b4q: a bank is eight tiles held twice

`$a3a6 lea $7656,a0; adda.w d5,a0` is the whole explanation. `$7656` is eight
cells past `$7616`, and `d5` is the struck cell's byte offset. Put it beside the
two index formulas — a disc's cell is `column(x + 4) + (4 if y > 70)`, 1..8; a
player's is `8 + column(x) + (4 if y > 14)`, 9..16 — and **cells 1..8 and 9..16
are the same eight tiles**: 1..8 the record the damage path writes, 9..16 the
copy the movement code reads for walkability. Hence hp 4–5 in the low eight and
a dummy hp of 1 in the high eight.

Destroying a low cell claims the single collapse-effect slot at `$779e`, points
it at the struck cell **plus eight**, and runs 48 frames of sprite animation off
the list at `$5be4`. The tick after that list runs out, `$14bb8 clr.w (a0)`
clears the high copy's type — and only its type, which is exactly the
`(1,1) → (0,1)` that looked impossible. **A tile's walkability survives its own
destruction by 49 ticks.**

There is **one** slot and no queue (`$a38c tst.b (a2); bne`), so a second tile
destroyed mid-collapse gets no animation and — since the clear lives inside the
animation — its walkability copy is never cleared at all. That is a game quirk,
faithfully modelled, not a corner cut.

## Each player has four throw states, two of them running smashes

`move.w #$51,d0`, the first instruction of every serve parameter build, appears
exactly **eight** times in the image: four per player. Player 2's are `$c6ec`
entries 3, 4, 15 and 16, and they are one routine written four times with three
constants swapped:

| state | gate | `world_x` | step | wind-up |
|---|---|---|---|---|
| 3 | `$4754` | `p2.x - $b` | 2 | slides left one unit a frame, then jumps `-$a` |
| 4 | `$471a` | `p2.x + 4` | 2 | slides right, then jumps `+$a` |
| 15 | `$4602` | `p2.x - 9` | 1 | none |
| 16 | `$45da` | `p2.x + 3` | 1 | none |

**3 and 4 are running smashes** — the disc leaves with twice the sideways step.
`tile_damage.ndjson` frame 190 is one: player 2 at x 85 puts the disc at 89 with
`vel_x` 4, where a standing throw gives 2. Part 10c had modelled only 15 and 16
and stopped there because those were the two the fixtures reached.

## A correction that cost exactly one frame

`docs/disc-notes.md` said the `$7bfe` column table is 152 bytes. It is **160** —
four blocks of forty — and 152 was a short dump. The consequence was precise: a
disc at `world_x` 151, which the `$9b` ceiling allows, reads index 155, and the
152-byte reading made `disc_cell` give up on exactly the frame
`tile_damage.ndjson` destroys a cell (208). Fixed, and the test now pins both
151 and the out-of-arena case.

## Honest limits on two clean runs

* **Two clean fixtures are two fixtures.** The fully-compared run still stops at
  21 ticks, on player 2's state 18. `disc-core` is right about everything these
  two traces do — 313 ticks of it — and that is not the same as being right about
  the game. `scripts/regen_golden.sh` and one oracle invocation make more.
* **Six ST fields are still fed every tick** and the header names them.
* **The far bank `$7596` has never changed in any trace**, so nothing tests it.
  The oracle emits both banks now (`banks`, 32 cells) but `tracecheck` still
  compares the 17-cell `grid`; making it compare 32 is discr-ovl.5.
* **The racket path is still unmodelled** (discr-ovl.1) and neither fixture
  reaches it, because neither player ever swings at a disc.
* **Player 1's four throw states are not transcribed.** They exist at `$fa0c`,
  `$fabe`, `$10770` and `$10806`; player 1 never throws in either fixture, so
  their constants are unread and the model would be untested.
* **The 48-frame collapse is one observation of one destruction.** The frame
  count is read from the `$5be4` list rather than fitted, which is stronger than
  n=1 usually is, but a second destruction has never been traced — and the
  single-slot behaviour above means a second one behaves differently anyway.

---

# Part 10f — the steering hook stops being fed

No number went up much this time. What changed is what the numbers *mean*:
`disc+$12` came off the fed-input list, so `disc-core` installs the steering
hooks itself, and the two clean fixtures stayed clean with the row **compared**
rather than supplied.

## The scoreboard

| run | 10d | 10e | **10f** |
|---|---|---|---|
| `golden --skip-waived` | 99 clean | 99 clean | **99 clean, with `hook` compared** |
| `tile_damage --skip-waived` | 118 | 214 clean | **214 clean, with `hook` compared** |
| `golden` (no flags) | 21 | 21 | **39** |

`discs[n].hook` is the 17th compared row. Both fixtures reproduce all 30 hook
installs and every clear, frame for frame, which is a much stronger statement
than 214 clean ticks with the hook handed over.

## What `$c826`'s tail turned out to be

Player 2's hit test is `$10fd8` mirrored — same crossing test, same owner check,
same racket path, same body box. What player 1's does not have is a **tail**, and
that tail is the hook installer discr-ovl.1 was asking about:

```
$cb52  d5 = own depth - own reach ($6d32; or $32 flat under bonus code 5)
$cb6a  the disc must be at least that deep -> otherwise nothing at all
$cb70  INSTALL $a7d8 -- start tracking it
$cb78  a two-unit-deep window inside that; outside it, stop here
$cb9e  a ladder on the disc's X either side of $6d22 - 3, mirrored
$cbae  REACH:     keep $a7d8, state 27
$cc1e  INTERCEPT: install $a816, state 18
```

**Tracking a disc and committing to a response are separate decisions** —
`--watch` counts 28 `$cb70` installs against one each of `$cbae` and `$cc1e`
across 215 frames.

The choice between them is a genuine little piece of judgement: `$cc02`/`$cc10`
**step across only if the cell twelve units over is somewhere you could stand**
— either the one you're on, or one whose type is non-zero in your own bank.
Both fixtures make the decision once, in opposite directions: `tile_damage`
frame 21 steps across, frame 111 reaches.

Also read: `$c196`, state 18's handler. Play the reach animation, and on the one
frame the cursor reaches `$4624`, if fire is still held and the two disc counters
differ, **step six units left in one go and go straight into a throw**.
`golden.ndjson` frames 39→40 are exactly that: cursor `$4624`, then x 63 → 57 and
state 15.

## One small change with a large effect

Every entry in either state table opens by stamping its own index into
`player+$09`. So `disc-core` stamps it once, up front, for **all 32 entries —
including the 25 whose behaviour is not modelled**. `players[n].facing` is now
correct for every state either player reaches, which is most of what took the
fully-compared run from 21 ticks to 39.

## The fed-input ledger, which is the point of this part

| | before 10f | after |
|---|---|---|
| `disc+$12` (the steering hook) | fed, changing 30 times | **produced and compared** |
| `disc+$10` bit 7 | fed | fed (discr-0fm) |
| `player+$3a` (animation cursor) | fed | fed (discr-75o) |
| `player+$1c..$22` (hit box) | fed | fed — sprite data (discr-75o) |
| `player+$6e`/`+$70` | fed | fed — per-player constants (discr-qqt) |
| `player+$12` (reach) | — | fed — a per-player constant (discr-b6x) |

Six fed fields either way, but the composition changed: the one that carried a
**decision** was replaced by one that carries a **constant**. `player+$12` reads
12 for player 1 and 26 for player 2 and is never written anywhere in the
analysed image.

## Honest limits

* **The right-hand half of the X ladder (`$cc40`-`$cc9a`) is transcribed but
  untested.** Neither fixture ever has a disc to player 2's right at the moment
  it starts tracking, so that branch has never run. It mirrors the left half
  instruction for instruction, which is why it is transcribed at all.
* **`$cb34`'s `$6d29 != 7` guard is not modelled.** That byte is stamped by the
  throw states and is never 7 in either fixture; inventing a value for it would
  be worse than recording that it is missing.
* **State 18's handler is not modelled**, which is where the fully-compared run
  now stops. It needs `$6d8a`/`$6d8c`, the possession counters the disc loop
  moves at four sites.
* **`tiles_far` (`$7596`) is seeded and read but never compared.** The
  anticipation cascade tests it for walkability, so `disc-core` now carries the
  bank — but no trace has ever changed a cell in it, so comparing it would prove
  nothing yet. discr-ovl.3.
* **Player 1's racket path is still unreachable** in both fixtures. `$113e2`
  fires zero times in 215 frames.

---

# Part 10g — the last disc-side fed input, and discr-0fm

Both fixtures still clean, and now with `disc+$10` **produced and compared**
instead of fed. Nothing on the disc side is supplied any more. The
fully-compared run went **39 → 58**.

## The scoreboard

| run | 10e | 10f | **10g** |
|---|---|---|---|
| `golden --skip-waived` | 99 clean | 99 clean | **99 clean, `active` + `hook` compared** |
| `tile_damage --skip-waived` | 214 clean | 214 clean | **214 clean, same** |
| `golden` (no flags) | 21 | 39 | **58** |

18 compared rows now. Both new ones — `discs[n].active` and `discs[n].hook` —
were fed inputs two parts ago.

## discr-0fm closed: the dwell was a caught disc

Open since Part 8, and `--watch 0x6e4e 0x6e4f` answered it in one run. All four
writers of `disc+$10`:

| PC | what |
|---|---|
| `$a9b8` | `st` — the serve claims a free slot |
| `$caae` / `$cb1e` | `addq.b #4` — player 2 catches it, from state 18 or 27 |
| `$a570` | `addq.b #4` — the round ended, clear the board |
| `$012588` | `subq.b #1` — the **render pass** counts a retired slot down |

`$ff + 4` is `$03` and the countdown's first step lands in the same tick as the
catch, so a caught disc reads 2, 1, 0 and its record never moves again. **The
"dwell at `world_z` 54" was a disc that had been caught** — not a `world_z`
phase, not an anomaly, nothing to do with the `$4f` bound.

The countdown lives in `$012582`, the routine that draws a live disc and counts
down a retired one. Nine phases of reading `$a4ea` were never going to find it;
one watch did.

Two more rules came with it. **Missing a catch falls through to the strike** —
`$cab8`/`$cb28` branch on to `$c934`, the mirror of `$110fc` — so reaching for a
disc and missing means being hit by it. And **`player+$0d` ends a round**:
`$a564 tst.b $6d2d; bne $a570` retires every disc in play when the other player's
flag is set, and `$f1b4` sets it three instructions after they enter the death
state. `golden.ndjson` frame 97 is exactly that.

## State 18's handler, and the one stub in 64 states

`$c196` commits the intercept: on the frame the animation cursor reaches `$4624`,
if fire is still **held** (a level, not the edge the walk handlers consume with
`bclr`), and down is not, and the disc count has not reached its cap, step six
units left in one move and go straight into state 15.

`$6d8c` is that cap, never written anywhere in the image: **4 for player 2 and 0
for player 1** — whose count is also 0, so player 1 can never throw from that
state, which is consistent with it never throwing in either fixture.

And a correction to Part 10f. That part said every handler stamps its own index
into `player+$09` and stamped it for all 32 entries. Comparing each of the 64
table entries with the next handler in address order finds exactly **one
four-byte stub per player**, and it is state 17 in both — `$1089a bra $f1c4` and
`$c192 bra $ac40`. It has no body and stamps nothing, which the fixtures show
plainly: player 2's `+$09` holds 15 for all seven frames it spends in state 17
after a throw. The universal stamp was right for 31 of 32 entries and wrong for
the one the fixtures spend most time in.

## The fed-input ledger

| | 10f | 10g |
|---|---|---|
| `disc+$12` | produced | produced |
| `disc+$10` | fed | **produced and compared** |
| `player+$3a` | fed | fed — the animation cursor (discr-75o) |
| `player+$1c..$22` | fed | fed — sprite-derived hit box (discr-75o) |
| `player+$6e`/`+$70`/`+$12` | fed | fed — per-player constants |
| `player+$6c` | — | fed — the disc cap, another constant |

**Nothing on the disc side is fed any more.** What is left is one animation
cursor, one block of sprite-derived box data, and four per-player constants that
nothing in the analysed image writes.

## Where the fully-compared run stops now, and why

Frame 59: player 2 should leave state 17 for state 0 and `disc-core` holds it in
17. Leaving state 17 is the shared animation tail running out, which needs the
sequence the *entering* state loaded — `$45f0` after an intercept, `$462e` after
a missed catch. `disc-core` carries hold counts for the four sequences
transcribed so far and no more.

That is the clear next step and it is structural rather than exploratory: give
`Player` the sequence it is running, not just a cell index, and transcribe the
handful of `$45xx`/`$46xx` tables the fixtures touch. Everything needed is in
the image and readable with `--disasm`.

## Honest limits

* **`player+$6a` (the disc count) is produced but its four possession sites are
  not.** `disc-core` moves it on a serve, a catch and a round end. The disc
  loop's four possession-transfer sites (`$a5ee`-`$a5fa`, `$a630`-`$a63c`) are
  not modelled, and no trace exercises them — no disc has ever changed hands.
* **State 19's catch (`$cad0`) is not modelled.** No fixture reaches state 19.
* **Player 2's strike and racket halves are not modelled**, only its catch and
  anticipation. They mirror `$10fd8`'s, which is modelled, so this is
  transcription work rather than discovery.
* **Two clean fixtures are still two fixtures.** The strictest run reaches 58 of
  99 ticks.

---

# Part 10h — the animation tables

The smallest gain of the phase and the one that unlocks the rest: **59** on the
fully-compared run, up from 58, and the machinery that makes every further
handler cheap.

## The scoreboard

| run | 10g | **10h** |
|---|---|---|
| `golden --skip-waived` | 99 clean | **99 clean** |
| `tile_damage --skip-waived` | 214 clean | **214 clean** |
| `golden` (no flags) | 58 | **59** |

One tick. What was actually built is the animation engine's missing half.

## A handler names a sequence by the cursor it loads

`$c1d4 lea $45f0,a1` picks a cell **partway into** the block that starts at
`$45ea`, and the sequence runs forward from there to the same zero terminator. So
a sequence is identified by the cursor a handler loads, not by a table base —
which is why `Player` now carries `anim_base` (the loaded address) beside
`anim_cell` and `anim_hold`, and why `Anim` is keyed by ST address.

Recovering the tables is mechanical once you know a real cell is `(plausible
frame pointer, small hold)`. The packed tables sit adjacent, so walking back from
a known cell stops on the previous table's terminator, and the hold tells them
apart: a real hold is 4, 6, 48 or 80, while a frame pointer read as a hold is
five figures. Eleven sequences transcribed, every one cross-checked against the
`lea` that loads it.

## `$45f0` is why state 17 ends when it does

Twenty frames — five cells of four — shared between state 15 and the state 17
that follows the serve. Golden spends twelve frames in 15 and seven in 17, and
the sequence runs out on the twentieth tick, which is where the tail writes state
0. **A serve loads no new sequence**: `$c068`'s release path is `bsr $a972;
move.b #$11,$6d2e; bra $ac40`, so state 17 inherits what state 15 was running,
and the throw animation finishing is what ends the follow-through.

With all the pieces in one place the whole machine is one sentence: a state loads
a sequence, its handler runs each frame, the sequence's holds pace whatever the
handler does, and the sequence running out *is* the transition. Nothing in it is
a timer.

## Also: states 16 and 17 came off discr-rf9

They were waived as "seen only in an oracle autopilot run, never in Hatari".
Player 2 spends much of both fixtures in them, both are modelled, and the four
throw states plus the state-17 stub cover them. The bead's remaining scope is
what states 3 and 4 do during their wind-up.

## Honest limits

* **One tick is one tick.** The next wall, frame 60, is player 2's state-16 entry
  shifting its `world_x` by eight — one more `$c6ec` handler nobody has read.
  From here the fully-compared number moves **one handler at a time**, and that
  is now a grind rather than a discovery: `--watch` the field, `--disasm` the
  writer, transcribe, measure.
* **States 3 and 4's wind-up movement is not modelled** — they slide the player
  a unit a frame and jump ten at one animation frame. Their *release* is, which
  is why `tile_damage` frame 190's smash reproduces.
* **`anim_tick` holds rather than guesses** when it meets a sequence address it
  has not transcribed. That is deliberate: a wrong length would desynchronise a
  state machine silently, and holding shows up as a visible divergence instead.
* Both fixtures are still the same two fixtures.

---

# Part 10i — locating the rest of player 2, and calling the phase

No code this part, and that is the point. The remaining work on player 2 is
**transcription, not discovery**, and this part pins it down to the instruction
so the next session starts from an address rather than from a search.

## Gates, unchanged

| run | |
|---|---|
| `golden --skip-waived` | **99 clean** |
| `tile_damage --skip-waived` | **214 clean** |
| `golden` (no flags) | **59** |

## Everything left on player 2's `world_x`

`--watch 0x6d22 0x6d24` over the golden programme is the complete list:

| PC | what | state |
|---|---|---|
| `$b038` | −3 per frame | state 1, walk left — modelled |
| `$b24e` | +3 per frame | state 2, walk right — modelled |
| `$c1d0` | −6 in one step | state 18's commit — modelled |
| `$abc6` | the idle path consuming `player+$1a` | **not modelled** |
| `$ae84` | −4 on entering state 16 | **not modelled** |

**`player+$1a` is an X delta authored in the animation data.** `$abbe`-`$abc6`
(and `$f110`-`$f118` for player 1) reads it, clears it and adds it to
`world_x` — so some movement lives in the sprite tables rather than in code.
Part 10b noticed this and it is still the only reason player 2's `world_x` moves
while it is standing still.

## The pattern every throw entry follows

Worth writing down once, because everything left is a variation on it. It
appears verbatim at `$cc02`, `$adf0` and `$ae54`:

```
d0 = own world_x +/- <offset>          ; $adce is +$d, $ae94 is -$26
if d0 outside 8..$98      -> not standable
d0 = colTable[d0] + 8 ; if own world_y > $3a: d0 += 4
if d0 == own grid_cell    -> standable
if own bank[d0] type != 0 -> standable
```

**The polarity differs by site**, which is the trap: `$cc1c` takes *standable*
to the intercept, `$ae0a` takes *not standable* to state 16. Reading one and
assuming the other is exactly the class of mistake this project has retracted
three times — the steering gate, `vel_y`, and the `$c0e8` doubling.

## Why I stopped here rather than pushing the number

Passing frame 60 needs both missing writers, and the second one needs the
`$ae2e` mirror of the probe above, which I read the wrong half of first. One more
handler would buy a handful of ticks. That is a legitimate grind and it is now a
*cheap* grind — `--watch` the field, `--disasm` the writer, transcribe, measure —
but it is not the same activity as the rest of this phase, and pretending
otherwise by half-landing a rule would leave the repo worse than a precise
handoff does.

What is genuinely finished is more interesting than what is left:

* **Both fixtures reproduce end to end** — 99 and 214 ticks, no divergence.
* **Nothing on the disc side is fed.** `disc+$10` and `disc+$12` are produced and
  compared; the disc loop, its four bounds, the tile impact, the collapse, the
  serve from four throw states, the steering hooks and the catch are all
  mirrored from the disassembly.
* **Eight beads closed this phase**: discr-217, discr-tan, discr-5w5, discr-dc0,
  discr-m4x, discr-fnl, discr-1q7, discr-xfw, discr-b4q, discr-0fm, discr-rf9.
* **Three retractions written down**, each the same shape: a correct reading of
  an instruction paired with an untested assumption about which way a branch
  fell.
* **Two new tools** that changed what is cheap: `scripts/ghidra/` and the
  oracle's `--watch` / `--disasm`.

## What a next phase should pick up, in order

1. **`player+$1a` and state 16's entry** — the two writers above. Cheap, and
   they take the fully-compared run past 59.
2. **The `$ae2e` mirror** and the other throw entries, one at a time.
3. **A trace where a bonus is picked up.** Five effects (`$9aa2`) are code reads
   with nothing to test them against, and one of them — code 4, the shield — is
   in the strike path `disc-core` already models.
4. **A trace where a player swings**, which is the only way to reach the racket
   path (`$113e2` fires zero times in 215 frames) and player 1's four throw
   states.
5. **`discr-st8`** — round init, scoring and win. `$aa50` and `$6c83` are
   untouched, and Part 10g found the round's *end* (`player+$0d` clearing the
   board) without its beginning.

---

# Part 10j — golden reproduced with nothing waived at all

`tests/fixtures/golden.ndjson` now runs **99 of 99 ticks with nothing waived and
nothing resynced** — every compared row of *both* players, including all five of
player 2's, which had never matched a single frame before Part 10c.

## The scoreboard, four gates

| run | what it measures | 10i | **10j** |
|---|---|---|---|
| `golden --skip-waived` | everything but player 2 | 99 clean | **99 clean** |
| `tile_damage --skip-waived` | the same, idle fixture | 214 clean | **214 clean** |
| `golden` (no flags) | **nothing waived at all** | 59 | **99 clean** |
| `tile_damage` (no flags) | the same, idle fixture | 59 | **161** |

Resyncing buys nothing on the golden fixture any more.

## Three small reads, and one bug of mine

**`$ad82`-`$ae2a` is how player 2 decides to throw.** Fire alone does nothing;
fire with a direction probes one side and commits: state 16 steps *left* and
throws, `$ae0e`'s state 15 steps *right*. The two probe arms reach state 16 from
opposite outcomes, which is one rule and not two — probing right and finding
nowhere to go means go left, probing left and finding somewhere to go also means
go left. `player+$08` records which way the last throw went and breaks the tie
when the stick is neutral. It also explains why the two serves offset
differently, `p2.x - 9` against `p2.x + 3`: those are measured *after* the
sidestep.

That corrects Part 10i, which called this "the polarity differs by site" and
filed it as a trap. It is not a trap; it is one decision read from two ends.

**`tst.b (a0)` is a whole-byte test and `$80` is not empty.** The idle path
clears `player+$09` only when the entire input byte is zero. A byte of `$80` —
fire held, no direction — is non-zero, so the stamp from whatever the player was
last doing stays put. `tile_damage.ndjson` frame 60 catches it exactly: the AI
holds `$80`, the byte keeps the 15 the throw left there, and treating "no
direction bits" as "no input" drops it to 0 on a frame the ST leaves alone. That
one line took the idle fixture's strict run from 59 to 134.

**`player+$1a` is an X delta the idle path consumes** — read, cleared and added
to `world_x` once per frame, copied out of the animation cell like the hit box.
It is the only reason a standing player's `world_x` moves, and nothing recomputes
`grid_cell` after it, so a probe on the same frame compares against the previous
frame's cell.

**And a bug worth recording, because it cost more than the reads did.**
`discs_out` and `disc_cap` were added to the oracle's *argument list* in Part 10g
but never to its *format string*. `fprintf` ignores extra arguments, so the
columns simply never appeared, both read 0, and `disc-core` could not tell "no
discs in play" from "at the cap" — which silently gated every throw decision
that consults it. It surfaced only when a decision that depends on the
difference finally mattered. A missing conversion in a `printf` is not a
compile error and not a runtime error; it is a column that quietly is not there.

## What is left on player 2

Both remaining walls are the same thing: the **running smash**. `$b1e0`-`$b1f8`
in the walk handlers send a fire press to `$ad82`, which sees the walk's own
stamp in `player+$09` (1 or 2) and routes to `$ae90` or `$aef0`, the choosers
for states 3 and 4. `tile_damage.ndjson` frame 162 is one: player 2 walking right
with fire enters state 4 and starts sliding a unit a frame toward the wall.

Its *release* is already modelled — that is why frame 190's smash serves
correctly with `vel_x` 4 — so what is missing is the entry and the wind-up slide.

## Honest limits

* **`player+$1a` is a seventeenth waived row**, fed like the hit box. The fed
  list is now six player fields: one animation cursor, the hit box, the X delta,
  and three per-player constants. Nothing on the disc side.
* **The `$aef0`/`$ae90` smash choosers and `$af50` (fire+down) are unread.**
* **Player 1's idle-path throw branch (`$f21e bmi $f306`) is unread**, and
  cannot be exercised: `disc_cap` is 0 for player 1 with a count of 0, so it can
  never throw.
* **One fixture clean under the strictest terms is one fixture.** The idle one
  reaches 161 of 214 the same way.

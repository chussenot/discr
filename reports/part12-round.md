# Part 12 (round): the four counters get a writer; round init, round-over and win/loss named without a fixture

bd `discr-st8`. Every claim below cites an ST address, and either a Ghidra
static disassembly, a raw `capstone` byte-pattern scan of `discram.bin` (for
code outside Ghidra's own CFG walk -- same technique as `reports/
part12-rng.md` and `part12-dirkind.md`), or a measured trace. No live Hatari
capture in this pass crosses a round transition, so nothing below about round
init, round-over or win/loss is dynamically confirmed -- flagged throughout.

## The question

`reports/part12-owner.md` (discr-ovl.2) pinned the four possession counters'
writer PCs and directions: serve (`$a9aa`/`$a9bc`) and the two wall handlers
(`$a5d0`-`$a5fa` far, `$a612`-`$a63c` near). It left three things open, all
this bead's: give the wall transfer a `disc-core` writer; decode round init
(`$aa50`, named in Part 10 but never traced to its caller); and decode what
ends a round and decides who won it.

## The four counters: now computed

`crates/disc-core/src/round.rs` (new): `transfer_at_far_wall` and
`transfer_at_near_wall`, called from `disc::step`'s two wall-bound match
arms (`next > Z_FAR` / `next < Z_NEAR`), each a plain four-field move:

```rust
pub fn transfer_at_far_wall(players: &mut [Player; 2]) {
    players[1].discs_out = players[1].discs_out.saturating_sub(1);
    players[1].disc_cap = players[1].disc_cap.saturating_sub(1);
    players[0].discs_out = players[0].discs_out.saturating_add(1);
    players[0].disc_cap = players[0].disc_cap.saturating_add(1);
}
```

`transfer_at_near_wall` is the exact mirror. Both are called from inside the
existing owner-byte branch (`if disc.aim == PlayerId::One { .. } else { .. }`
at the near wall; a NEW such branch added at the far wall, which previously
had none), so no new comparison against `disc.aim`/`PlayerId` was
introduced -- see "What was NOT changed" below.

Wired into `docs/state-schema.md` and `crates/disc-tools/src/main.rs`:
`players[0].discs_out`/`disc_cap` moved from waived to compared (`checks()`),
and the unconditional `disc_cap` feed in `feed_disc_inputs` is gone (`discs_out`
was never fed to begin with -- only fed via the frame-0 seed either way).

**Measured**: `tests/fixtures/p1_walk.ndjson`, `--min-agree 274` (the whole
275-frame trace), reaches the far-wall transfer live at frame 220
(`disc[0].own` 0 -> 255) with both fields now genuinely computed rather than
fed or silently uncompared, and the gate holds:

```
$ cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/p1_walk.ndjson --min-agree 274
docs/state-schema.md: 20 compared, 16 waived, 5 excluded
...
OK: 274 tick(s) matched, no divergence.
```

`tests/fixtures/handover.ndjson` records BOTH directions on one disc slot
(frame 259 far wall, frame 339 near wall -- `reports/part12-owner.md`), but
`disc-core`'s own replay of it diverges earlier (tick 222 `--skip-waived` /
tick 21 bare) on an unrelated `discs[0].active` gap in the retirement
countdown, before frame 259. So `transfer_at_near_wall` is measured against
the fixture's own recorded columns (frame 339: `players[0]` 1,2 -> 0,1,
`players[1]` 0,2 -> 1,3), not against a tracecheck run that reaches it live --
documented as such in `round.rs`'s own module docs, not overstated.

**Not modelled**: the `$6ca0.b != 1` gate on both wall transfers. `reports/
part12-owner.md` flagged this byte as "an apparent flag on player 1's own
record", undecoded. This part names it (see below): `$6ca0` is the game-mode
byte, and the wall transfer is gated OFF entirely in training. No fixture on
hand is a training-mode capture with a live wall crossing, so `disc-core`
does not gate on it and would over-transfer in training.

## Where `$aa50` sits: the round-start init chain

`$aa50` was named in Part 10 (`docs/disc-notes.md` line ~559: "round
initialiser: 8 disc records + their sub-records") but never traced to a
caller. Ghidra's own analysis agrees it has never disassembled the caller:

```
$ ./scripts/ghidra/q.sh xref aa50
=== XREF to 0000aa50 === (0 reference(s))
$ ./scripts/ghidra/q.sh scan aa50
=== SCAN for operand aa50 === (0 instruction(s))
```

A raw byte-pattern scan of `discram.bin` for `BSR.W`/`BSR.B` opcodes whose
computed target is `$aa50` (Python + `capstone`, disassembling forward from
each hit to confirm) finds exactly one: a `bsr.w` at `$965c`. It sits inside
a contiguous, uninterrupted `capstone` disassembly from `$9628` to `$96b6`
that is unmistakably round-start init -- it falls straight through into the
already-known main VBL loop at `$96ba` (`move.w $6ab8,-(a7)`, the exact
instruction `crates/disc-core/src/lib.rs`'s own module docs already cite as
`GameState::update`'s entry):

```
$9628  jsr      $11592.l
$962e  bsr.w    $92b6
$9632  jsr      $154dc.l
$9638  jsr      $115d4.l
$963e  bsr.w    $9e92
$9642  bsr.w    $a208
$9646  bsr.w    $a238
$964a  bsr.w    $a224
$964e  bsr.w    $a4d6
$9652  jsr      $123be.l
$9658  st.b     $6c4a.w      ; "round active" flag -- see $97ea below
$965c  bsr.w    $aa50        ; <<< the disc-array reset
$9660  bsr.w    $ab84
$9664  bsr.w    $ce7e
$9668  jsr      $11684.l
$966e  bsr.w    $1147e
$9672  bsr.w    $ccba
$9676  bsr.w    $9a52
$967a  bsr.w    $99a8
$967e  bsr.w    $9a1a
$9682  clr.b    $6c83.w      ; <<< the round-over tally, see below
$9686  clr.w    $6c9c.w
$968a  move.b   $6ab5.w, $6c5d.w   ; the already-known Part 12b RNG reseed
$9690  clr.w    $6ab8.w
$9694  clr.b    $6c5a.w
$9698  move.w   #$0, $6ab6.w
$969e-$96b6  seven more bsr.w calls to other subsystem init
$96ba  move.w   $6ab8.w, -(a7)     ; <<< the known main loop entry
$96be  bsr.w    $a4ea              ; the disc loop -- GameState::update
$96c2  bsr.w    $10eac             ; player dispatch
```

`$aa50` itself, disassembled in full (`scripts/ghidra/q.sh dis aa50 100`):

```
$aa50  lea (0x6e3e).w,A5        ; disc array base
$aa5a  move.w #0x3,(0x18,A5)    ; disc+$18 := 3 -- NOT a modelled field
$aa60  move.w #0x52,(0x2,A5)   ; world_y := 0x52 (82)
$aa66  clr.b  (0x11,A5)         ; owner := 0
$aa6a  clr.b  (0x10,A5)         ; active := 0 (free)
$aa6e  move.l #0x4a46,(0x1a,A5) ; the excluded pointer at +$1a
$aa76  move.l A0,(0x3e,A5)      ; the excluded pointer at +$3e
   ... (A0's own sub-record init, two 0x1e-strided entries)
$aa8e  lea (0x42,A5),A5          ; next of 8 disc records
$aa92  dbf D0,$aa5a
```

`disc+$18` (a word, right after `damage` at `+$16` and before the excluded
pointer at `+$1a`) is a previously undocumented field this crate does not
carry. Flagged, not chased further: finding its other reads/writes needs a
full disassembly of the surrounding register-relative code, which offset
scanning (this report's own technique) cannot do -- it only finds absolute
operands, not `(0x18,A5)`-style accesses, which recur for every disc slot at
a different runtime address.

## The round-play loop, the round-over threshold, and the mode byte

`$9600`-`$97d6` (also read via `capstone`, since it too sits outside Ghidra's
existing analysis) is the per-VBL loop that plays one round:

* `$9700`-`$972e`: the VBL-pacing wait, computing `$6ab8` -- the pass count
  `GameState::updates` already models from the trace side (Part 11f).
* `$96ba`-`$96cc`: the already-known main update (`$a4ea` disc loop, `$10eac`
  player dispatch, both `GameState::update`'s own calls).
* `$9746`-`$97cc`: the round-over/win-check block, below.
* `$97d0  cmpi.b #1,$6c56; bne.w $9698`: loop back for another frame unless
  `$6c56 == 1` (not decoded, not chased -- outside this bead's scope).

`$6c83` is a GLOBAL counter, not per-player: bumped +1 by each player's own
state-23 terminal handler. A raw scan for `addq.b #1,$6c83` finds exactly two
sites:

```
$c3b6   addq.b #$1,$6c83.w    ; player 2's own state-23 mirror
$10abe  addq.b #$1,$6c83.w    ; player 1's, already named in docs/disc-notes.md's
                              ; state-23 section ("bumps $6cab by 3 and $6c83 by 1")
```

`docs/disc-notes.md`'s state-31 section (Part 12, already committed) found
that state 31 -- the sustained-upward-jump exploit -- forces the *same* two
flags state 23's own death path sets, and falls through into state 23's
terminal code once its own animation sequence runs dry. So both of this
project's known round-ending triggers, energy death and the jump exploit,
fund the same `$6c83` counter; neither is a separate round-end mechanism.

`$9746`-`$975c` reads `$6c83` against a threshold gated by `$6ca0`, read as a
BYTE at player 1's own base address (`+$00` -- distinct from any modelled
`Player` field, which starts at `+$02` with `world_x`):

```
$9746  cmpi.b #1,$6ca0
$974c  bne.b  $9756
$974e  tst.b  $6c83          ; training: ANY death ends the round
$9752  bne.w  $97ea
$9756  cmpi.b #2,$6c83        ; else (challenge/tournament): TWO deaths
$975c  bge.w  $97ea
```

**`$6ca0` is the game-mode byte.** `cmpi.b #1,$6ca0` recurs at `$9746` and
again at `$97a8` in this same loop; independently, `docs/disc-notes.md`'s
discr-qqt section already established `$116c4` sets `$6c60 >= 16` "only when
the mode byte `$6ca0` selects training". Three unrelated call sites agree: 1
selects training. This **retracts** `reports/part12-owner.md`'s framing of
the wall transfer's `$6ca0.b != 1` gate as "an apparent flag on player 1's
own record" -- it is the mode selector, not a per-record flag, and the wall
transfer (and the round-over threshold) are both gated by it. Training ends
a round in one death; challenge/tournament need two.

## Win/loss: a previously undocumented field at `player+$72`

`$97b2`-`$97cc`, reached once a latch (`$6c9c`) permits it:

```
$97b2  move.w $6d12,d0
$97b6  cmp.w  $6d92,d0
$97ba  beq.b  $97d0            ; equal -> no winner marked
$97bc  blt.b  $97c8
$97be  st.b   $6cad            ; player 1's round_over
$97c2  st.b   $6d2c            ; player 2's down
$97c6  bra.b  $97d0
$97c8  st.b   $6d2d            ; player 2's round_over
$97cc  st.b   $6cac            ; player 1's down
```

`$6d12` is player 1's base (`$6ca0`) + `$72`; `$6d92` is player 2's base
(`$6d20`) + `$72` -- one word past `throw_damage` (`+$70`), previously
undocumented (no row in `docs/state-schema.md` before this pass). Whichever
side is SMALLER gets `down` set on its OWN record, and the LARGER side gets
its own `round_over` set -- consistent with `round_over`'s already-established
meaning ("my opponent is out, I should stop too", `crates/disc-core/src/
types.rs`'s own doc comment).

A raw scan for writers of `$6d12`/`$6d92` finds a BCD-style
incrementer-with-carry:

```
$9938  addq.w #$1,$6d14         ; +$74, a second word
$993c  lea    $6d14,A0
$9940  addq.b #$1,-(A0)          ; predecrement, bump the digit below
$9942  cmpa.l #$6d12,A0
$9948  beq.b  $9950              ; reached the top digit -- stop
$994a  cmpi.b #$a,(A0)           ; overflowed past 9?
$994e  beq.b  $9952
$9950  rts
$9952  clr.b  (A0)               ; carry: reset this digit
$9954  bra.b  $9940               ; and bump the one above it
```

Mirrored at `$9956` for player 2 (`$6d94`/`$6d92`). A display-ready
multi-digit counter with carry propagation reads as a SCORE, but nothing on
hand ties it to a specific in-game event: no fixture's `player+$72` ever
moves across either recorded match. Named by the writer's shape, not by an
observed increment -- flagged as unconfirmed.

## The round-over exit, and where the FDC boundary sits

```
$97ea  clr.b $6c4a          ; the "round active" flag $9658 set
$97ee  jsr   $12092.l
$97f4  clr.b $6c4a
       [one more VBL wait]
$980e  rts
```

`reports/part10-report.md` found the round's end without its beginning; this
part finds the beginning (`$9628`-`$96b6`) and the counter/threshold that
triggers the end, but `$97ea`'s own caller -- whatever loads the next round or
the match summary off floppy -- is outside every snapshot this project has
taken. That caller is where the FDC boundary actually sits: one level above
anything disassembled in this pass, not at `$97ea` itself.

## What was NOT changed, and why

No new comparison against `disc.aim`/`PlayerId` was added. The near wall
already branched on `disc.aim == PlayerId::One`; the counter movement for
its "else" (transfer) arm is a side effect inside an already-selected branch,
not a new comparison, so it carries no polarity risk. The far wall previously
had NO owner branch at all (just an unconditional `dir_kind` negate); this
pass adds `if disc.aim == PlayerId::One { transfer_at_far_wall(players); }`
around the counter move only, leaving the existing unconditional negate (and
the still-unmodelled force/damage arm, discr-ovl.3) untouched. This is a
structural change to that match arm, and it lands on a worktree whose
`disc.rs` predates farbank's wave-4 commit `096467a` (the coordinated
`PlayerId` polarity flip) and `9bd9a00` (the far wall's own tile-grid
comparison, discr-ovl.3, closed on that same branch) -- both unreachable from
this worktree (`git merge`/`cherry-pick` from a worktree onto the shared
branch is refused by this fleet's own tooling; merges here are the
orchestrator's, from the main checkout). Flagged to `farbank` via `pact msg`
at task start; the two branches' far-wall restructuring will need reconciling
at merge time, same as the flip itself.

## Gates

```
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test   # clean
tracecheck golden.ndjson       --skip-waived --min-agree 99   -> OK, 99
tracecheck tile_damage.ndjson  --skip-waived --min-agree 214  -> OK, 214
tracecheck golden.ndjson       --min-agree 99                 -> OK, 99
tracecheck tile_damage.ndjson  --min-agree 214                -> OK, 214
tracecheck p1_walk.ndjson      --min-agree 274                -> OK, 274 (discs_out/disc_cap now COMPARED, not fed)
tracecheck handover.ndjson     --min-agree 21                 -> PASS, 21  (unchanged, players[1].state_index gap)
tracecheck handover.ndjson     --skip-waived --min-agree 222  -> PASS, 222 (unchanged, discs[0].active gap)
tracecheck bonus.ndjson        --skip-waived --min-agree 150  -> PASS, 150 (unchanged)
tracecheck bonus.ndjson        --min-agree 22                 -> PASS, 22  (unchanged)
tracecheck farbank.ndjson      --skip-waived --min-agree 34   -> UNAVAILABLE: fixture does not exist in this
                                                                  worktree (farbank's wave-4 fixture, unmerged
                                                                  branch -- see "What was NOT changed" above)
cargo clippy -p disc-app --all-targets -- -D warnings  # clean
cargo test -p disc-app                                 # 5 passed
```

No existing number shrank; `p1_walk` now exercises real computation where it
previously exercised nothing (`discs_out`/`disc_cap` were neither fed nor
compared before this pass -- see `crates/disc-tools/src/main.rs`'s `checks()`
before this diff, which never pushed either field).

## Files touched

* `crates/disc-core/src/round.rs` (new) -- the two transfer functions + tests
* `crates/disc-core/src/lib.rs` -- `pub mod round;`
* `crates/disc-core/src/disc.rs` -- both wall arms call into `round::`; three
  stale doc comments corrected (`Z_FAR`'s "no trace reaches it", the
  near/far-wall "polarity not settled" module note, both predating Part 12)
* `crates/disc-tools/src/main.rs` -- `discs_out`/`disc_cap` added to
  `checks()`/`resync()`; the `disc_cap` feed removed from `feed_disc_inputs`;
  `SCHEMA_COMPARED`/`SCHEMA_WAIVED` and header text updated; one test's
  hardcoded check count corrected (117 -> 121)
* `docs/state-schema.md` -- two new compared rows, the standalone
  `disc_cap` waiver removed, the round/score/win waiver reworded, waived-row
  counts corrected (22 -> 21, 17 -> 16)
* `docs/disc-notes.md` -- one appended section (this report's summary)
* `scenarios/round_watch.yaml` (new) -- unrun scaffold for a future live
  round-transition capture
* bd: comment on `discr-st8`; this report

## What a future fixture needs

A live Hatari watch armed on `$6c83`/`$6ca0`/`$6d12`/`$6d92`/`$6c4a` from
mid-round through the `$97ea` exit and into whatever comes next --
`scenarios/round_watch.yaml` is the scaffold, not yet run to completion.
`scripts/ramdiff.py` on a `dump: pre`/`dump: post` pair bracketing the
transition would catch every field it writes, not just the ones this pass
already guessed (`disc+$18`'s meaning, `$6c56`'s role in the loop-continue
check, and whatever `$97ea`'s caller does next, are all still open).

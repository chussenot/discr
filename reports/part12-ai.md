# Part 12 — decoding `$d2cc`: the opponent's policy, as far as it goes

The brief (bd discr-b6x) was to decode player 2's AI — the 20-entry rule
table at `$efa8`, its writer `$d2cc`, and the sensor pass `$cea6` — and turn
it into a Rust `Ai` in `disc-core`, measuring agreement against the three
committed fixtures rather than guessing.

The table is now fully decoded: not "~20 rules" but exactly 20, every
priority and threshold read straight from the ST image, every routine
address named. The dispatch mechanism (the reaction roll, the priority
latch, the "plan" mini-VM its actions compile into) is fully decoded too,
byte-verified against the raw memory image, not just Ghidra's disassembly
view. Two of the twenty rows are decoded and implemented end to end. The
other eighteen are decoded exactly as far as the point where they stop being
decodable: their reaction roll reads a byte, `$6c5d`, that no fixture
carries and that (argued below) cannot be reconstructed from one. That is
not a shortage of effort; it is where the evidence actually runs out, cited
with its own address like everything else here.

Every claim below ties to an ST address and a command that produced it.
Where a command reads *data* rather than *code*, Ghidra's own instruments
cannot do it (`dis`/`dec` walk instructions, and a data address either isn't
one or resolves to whatever real instruction follows it — this cost an hour
of confusion on `$efa8` itself, see below) — those reads are a raw byte pull
from `discram.bin` at the cited offset, cross-checked against the
instruction that consumes the bytes.

    mkdir -p /tmp/ghidra-ai && cp -r tmp/ghidra_proj /tmp/ghidra-ai/proj
    GHIDRA_HOME=tmp/ghidra_12.1.3_PUBLIC GHIDRA_PROJ=/tmp/ghidra-ai/proj \
        ./scripts/ghidra/q.sh dis d2cc 250 xref 6da2 xref efa8
    python3 -c "open('/tmp/ghidra-ai/proj/discram.bin','rb')..."   # table + tables
    cargo test -p disc-core --lib ai::agreement -- --nocapture     # the numbers

## The scoreboard

| what | status |
|---|---|
| `$d2cc`'s dispatch loop (reaction roll, priority latch, plan/identity call convention) | **fully decoded**, byte-verified |
| The 20-entry table at `$efa8` | **fully decoded** — every address, priority, threshold |
| The sensor pass `$cea6` | **fully decoded** as *code* (exact registers/memory it touches); not needed by, and not used by, entries 0 or 1 |
| The "plan" mini-VM (`$e214`/`$e30a`/`$e2d0`/`$e290`/`$e2ac`/`$e2b4`) | **fully decoded** |
| Entry 0 — the escape (`$e0d8`) | **fully decoded and implemented** |
| Entry 1 — the avoid (`$e158`) | **fully decoded and implemented** |
| Entries 2–4 — disc pursuit, `aim`-gated | **decoded**; provably unreachable by any trace this project has (below) |
| Entry 5 — disc pursuit, the likely everyday rule | **test decoded**; action's output computation (`$d264`) not finished |
| Entries 6–17 (the twelve-entry walk cascade) | **not decoded** |
| Entries 19–20 | **partially read** (both are PRNG retry loops; not finished) |
| `disc-core::ai::Ai` | implements rows 0 and 1 only, by necessity — see "The wall" |
| Measured agreement | golden 18/99, tile_damage 61/214, p1_walk 22/200 (below) |

`mise run core-check`'s five gates are unaffected — this phase adds a module,
it does not touch `player.rs`/`disc.rs`/`tile.rs`/`lib.rs`'s `tick`, and
`Ai` is never wired into `GameState::tick`. The waiver (discr-b6x) stays
open; the reasons are the wall below, stated precisely rather than papered
over.

## Method note: `dis`/`dec` cannot read `$efa8` — it isn't code

The first attempt, `./scripts/ghidra/q.sh dis efa8 320`, printed disassembly
starting at `$f104` — a wrong answer with no error. `Q.java`'s `dis` calls
`getInstructionAt(addr)`, and when that is null (because the address is
data) falls back to `getInstructionAfter(addr)`, silently returning whatever
real instruction comes next. `$efa8` is exactly this: the rule table is
data, not code, so neither `dis` nor `dec` can see it at all.

The fix was to read the underlying RAM snapshot directly. `discram.bin` (in
the copied Ghidra project) is a flat 1 MiB image where file offset equals ST
address — checked by reading 16 bytes at `$d2cc` and diffing against the
disassembly's own opcode bytes, byte for byte:

    python3 -c "
    with open('discram.bin','rb') as f:
        f.seek(0xd2cc); print(f.read(16).hex())"
    # 41f86da1 4210 6100fbd2 2c786da2 4a56 ...
    #  lea $6da1,a0; clr.b (a0); bsr $cea6; movea.l $6da2,a6; tst.w (a6)

Every table row and every raw table (`$1556`, `$155e`, `$15fe`) below came
from this same read, not from Ghidra's listing.

## `$d2cc`: the dispatch loop

```
$d2cc  lea    $6da1,a0        ; a0 = &$6da1, the output byte -- kept live
$d2d0  clr.b  (a0)            ;   for the rest of the function
$d2d2  bsr    $cea6           ; the sensor pass (below) -- A5/D1/D2 out
$d2d6  movea.l $6da2,a6       ; a6 = the table head. $6da2 is DATA, not
                              ; code: xref shows it holds the constant
                              ; $efa8, never written by any instruction --
                              ; it is link-time initialised, not computed.
loop:
$d2da  tst.w  (a6)            ; a row's first word is (priority:8,thr:8);
$d2dc  beq    done            ;   0 means "past the last row"
$d2de  clr.w  d6              ; d6 = priority
$d2e0  clr.w  d2              ; d2 = threshold
$d2e2  move.b (a6)+,d6        ; row layout, 14 bytes:
$d2e4  move.b (a6)+,d2        ;   priority:u8, threshold:u8,
$d2e6  movea.l (a6)+,a2       ;   test:fn, action:fn, identity:fn
$d2e8  movea.l (a6)+,a3
$d2ea  movea.l (a6)+,a4
$d2ec  cmpa.l $6da6,a4        ; a4 == the CURRENTLY LATCHED identity?
$d2f0  beq    next            ;   yes -- this row is already running,
                              ;   its per-frame call happens once below
                              ;   the loop, not here; skip re-testing it
$d2f2  cmp.w  $6daa,d6        ; this row's priority > the latched one?
$d2f6  ble    next            ;   no -- can't preempt, skip
$d2f8  clr.w  d0
$d2fa  move.b $6c5d,d0        ; THE REACTION ROLL:
$d2fe  add.b  $6ab5,d0        ;   d0 = $6c5d + $6ab5 (mod 256),
$d302  move.b d0,$6c5d        ;   written back UNCONDITIONALLY --
$d306  cmp.w  d2,d0           ;   every row that reaches here pays this,
$d308  bgt    next            ;   whether or not it goes on to fire
$d30a  jsr    (a2)            ; the roll passed -- run the TEST
$d30c  tst.w  d0
$d30e  bne    next            ; test failed (d0 != 0)
$d310  lea    $6dac,a1        ; the "plan" buffer -- see below
$d314  move.l a1,$6dfc
$d318  jsr    (a3)            ; the ACTION compiles a plan into $6dac
$d31a  move.l a4,$6da6        ; LATCH: this row's identity is now current
$d31e  move.w d6,$6daa        ; LATCH: and its priority
next:
$d322  bra    loop
done:
$d324  tst.l  $6da6           ; is any row latched?
$d328  beq    rts
$d32a  movea.l $6dfc,a1       ; a1 = the plan cursor
$d32e  movea.l $6da6,a2       ; a2 = the latched identity
$d332  jsr    (a2)            ; run it -- ONCE A FRAME, latched or fresh
$d334  rts
```

Three findings worth stating on their own:

* **The reaction roll runs before the test, not gated by it.** A row's
  `$6c5d` increment happens for every row whose priority exceeds the
  current latch, regardless of what its test would have said — so `$6c5d`'s
  evolution depends only on which rows were *eligible* each frame, not on
  geometry. This matters for the wall, below.
* **A `u8` reaction roll can never fail a threshold of 255.** `d0` is
  built as `clr.w d0; move.b ...,d0`, so it is 0..=255, and `cmp.w
  d2,d0; bgt` can only branch when `d0 > d2`. Set `d2 = 255` (the
  threshold) and that branch is unreachable. Exactly two of the twenty
  rows carry threshold 255 — see the table below — and they are the only
  two whose firing does not depend on `$6c5d`.
* **Rows 0 and 1 share an identity pointer.** `$d2ec`'s check compares
  identity *function pointers*, and both rows point at `$e290`. So once
  either fires, the other cannot preempt it — `$d2ec` sees "already
  latched" and skips straight to `next` — until whichever one is running
  ends on its own. `disc-core::ai::Ai` models this with one `motive`
  field, not two.

## The table at `$efa8`

20 rows, 14 bytes each (`priority:u8, threshold:u8, test:fn, action:fn,
identity:fn`), terminated by an all-zero 21st row at `$f0c0` — read straight
from `discram.bin`:

| # | addr | priority | threshold | test | action | identity |
|---|---|---|---|---|---|---|
| 0 | `$efa8` | 50 | 255 | `$e0d8` | `$e214` | `$e290` |
| 1 | `$efb6` | 30 | 255 | `$e158` | `$e214` | `$e290` |
| 2 | `$efc4` | 20 | 200 | `$d4ea` | `$d6a2` | `$e222` |
| 3 | `$efd2` | 20 | 150 | `$d554` | `$d672` | `$e222` |
| 4 | `$efe0` | 10 | 100 | `$d5fe` | `$d672` | `$e222` |
| 5 | `$efee` | 12 | 230 | `$d6b4` | `$d6da` | `$e244` |
| 6 | `$effc` | 10 | 90 | `$dd68` | `$deea` | `$e274` |
| 7 | `$f00a` | 10 | 90 | `$dd68` | `$df58` | `$e274` |
| 8 | `$f018` | 10 | 90 | `$de8e` | `$deea` | `$e274` |
| 9 | `$f026` | 10 | 90 | `$de8e` | `$df58` | `$e274` |
| 10 | `$f034` | 10 | 90 | `$de12` | `$deea` | `$e274` |
| 11 | `$f042` | 10 | 90 | `$de12` | `$df58` | `$e274` |
| 12 | `$f050` | 10 | 90 | `$ddd4` | `$deea` | `$e274` |
| 13 | `$f05e` | 10 | 90 | `$ddd4` | `$df58` | `$e274` |
| 14 | `$f06c` | 10 | 90 | `$ddc4` | `$deea` | `$e274` |
| 15 | `$f07a` | 10 | 90 | `$ddc4` | `$df58` | `$e274` |
| 16 | `$f088` | 10 | 90 | `$da84` | `$deea` | `$e274` |
| 17 | `$f096` | 10 | 100 | `$da84` | `$df58` | `$e274` |
| 18 | `$f0a4` | 9 | 60 | `$da04` | `$df1c` | `$e274` |
| 19 | `$f0b2` | 8 | 50 | `$dff6` | `$e04a` | `$e290` |
| — | `$f0c0` | 0 | 0 | terminator | | |

(Part 10's "20-entry table" and "priority, a reaction threshold out of 255"
were exactly right; what it did not have was these addresses, because
Ghidra could not read them either.)

Reading the table: 14 distinct test routines, 8 distinct actions, 4
distinct identities — not the "11 tests, 7 actions" Part 10 estimated by
eye. Rows 6–17 are a 6×2 grid: six position tests (`$dd68`/`$de8e`/`$de12`/
`$ddd4`/`$ddc4`/`$da84`), each paired with one of two actions (`$deea`/
`$df58`), all sharing identity `$e274` — a twelve-entry cascade, structurally
alike the anticipation cascade `docs/disc-notes.md` already describes for
player 1's own hit test. Not transcribed this phase (see "What's left").

## The sensor pass, `$cea6`

```
$cea6  lea    $6e3e,a2         ; disc slot 0
$ceaa  suba.l a5,a5            ; a5 = 0 (no candidate yet)
$ceac  move.w $6d26,d1         ; d1 = player 2's own world_y
$ceb0  moveq  #-1,d2           ; d2 = best-so-far sentinel
$ceb2  moveq  #7,d0            ; 8 slots
loop:
$ceb4  tst.b  $10(a2)          ; slot active?
$ceb8  bpl    next             ;   no -- skip
$ceba  cmp.w  $4(a2),d1        ; d1(own world_y) > disc.world_z ?
$cebe  ble    next             ;   no -- skip
$cec0  tst.w  $a(a2)           ; disc+$0a (dir_kind) negative?
$cec4  bmi    next             ;   yes -- skip
$cec6  cmp.w  $4(a2),d2        ; disc.world_z > best-so-far?
$ceca  bge    next             ;   no -- skip
$cecc  move.w $4(a2),d2        ; new best
$ced0  movea.l a2,a5           ; a5 = this disc
next:
$ced2  lea    $42(a2),a2       ; next slot (stride $42)
$ced6  dbf    d0,loop
$ceda  rts
```

This picks, among the active discs with `dir_kind >= 0` whose `world_z` is
still less than player 2's own `world_y`, the one with the *largest*
`world_z` — and leaves it in **a5**, with **d1** (player 2's own `world_y`)
also still live. Both survive, untouched, all the way past the reaction-roll
loop (which reloads `d2`/`d6` from the table but never touches `a5`/`d1`) to
whichever row's test finally runs — which is exactly the calling convention
rows 2–5's tests use (`cmpa.l #0,a5` as their first instruction). **Neither
row 0 nor row 1 reads `a5` or this `d1` at all** — row 1 does its own
independent 8-slot scan (below) — so this pass, while fully decoded as
*code*, plays no part in what `disc-core::ai::Ai` implements. What it is
*for* (what a "disc with `dir_kind >= 0`, still short of my own row" means
tactically) is not claimed here, because nothing in these fixtures exercises
rows 2–5 to check it against (next section).

## The plan mini-VM

Row 0 and row 1's shared action (`$e214`) and step executor (`$e30a`),
decoded in full — this is the mechanism Part 10 described as "a rule's
action compiles a two-parameter plan... its identity routine executes the
plan once a frame":

```
$e214  move.l #$e30a,(a1)+     ; the plan buffer ($6dac) gets ONE step:
$e21a  move.w d1,(a1)+         ;   [$e30a, target_x, target_y, 0]
$e21c  move.w d2,(a1)+         ;   (d1/d2 are whatever the test left them
$e21e  clr.l  (a1)+            ;   as -- see rows 0/1's tests, below)
$e220  rts
```

```
$e290  tst.l  (a1)             ; the GENERIC identity: a1 = the plan
$e292  beq    end              ; cursor; its first longword is a step fn
$e294  movea.l (a1),a2         ; -- if one is there, run it
$e296  jsr    (a2)
$e298  rts
end:
$e29a  clr.l  $6da6            ; no step left -- end the maneuver:
$e29e  clr.w  $6daa            ;   un-latch, and reset the plan cursor
$e2a2  move.l #$6dac,$6dfc
$e2aa  rts
```

`$e30a`, the one step type rows 0/1 ever compile, walks player 2 toward
`(target_x, target_y)`:

```
$e30a  move.w 4(a1),d0         ; d0 = target_x, d1 = target_y
$e30e  move.w 6(a1),d1
$e312  bsr    $e2d0            ; is the TARGET cell still walkable? (below)
$e318  beq    end              ; ($e2d0 returns d2 = -1 on "no")
$e31a  move.w $6d22,d2         ; d2 = player 2's own world_x
$e31e  move.w $6d26,d3         ;      world_y
        ; d0 vs d2: target right of self -> RIGHT ($08), left -> LEFT ($04)
        ; d1 vs d3: target below self  -> UP    ($01), above -> DOWN ($02)
$e33a  cmpi.b #$0d,$6d2e       ; player 2's OWN state -- busy? (four values,
$e342  cmpi.b #$0e,$6d2e       ;  $0d/$0e/$18/$19) -- if so, clear the byte
$e34a  cmpi.b #$18,$6d2e       ;  and END the maneuver ($e372)
$e352  cmpi.b #$19,$6d2e
        ; within [target-4,target+4] x and [target-2,target+2] y? -> arrived,
        ; clear the byte, advance the plan cursor past this step ($e372/$e374)
        ; -- else: RETURN with the direction bits already set, unchanged
end:
$e372  clr.b  (a0)             ; a0 is STILL $6da1 -- live since $d2cc's
$e374  addq.l #8,$6dfc         ;   very first instruction
$e378  rts
```

`$e2d0` (the per-frame re-validity check — called every frame the step
runs, not just when it is chosen):

```
$e2d0  clr.l  d2                       ; d2 = 0 (assume OK)
$e2da  move.b $7bfe(d0.w),d0           ; colTable[target_x]
$e2e2  cmp.w  #$3a,d1 ; bgt +4          ; +4 if target_y <= $3a (58)
$e2ec  subi.w #9,d0                    ; -> floor-cell index 0..7
$e2f0  bmi    ret                      ; negative -> leave d2 = 0 (no block)
$e2f8  move.w $759e+2(d0*8),d0         ; tiles_far[cell].hp
$e2fc  andi.w #$7f,d0
$e300  bne    ret                      ; nonzero HP -> still walkable, d2=0
$e302  moveq  #-1,d2                   ; HP is 0 -- target has collapsed
ret:   rts
```

`$d062` (used by row 1's side-step probes, below) is the *same formula*,
written out a second time at a different address — not shared code, two
independent transcriptions:

```
$d062  lea    $7bfe,a2 ; clr.w d2 ; move.b (a2,d1.w),d2
$d06c  cmp.w  #$3a,d0 ; bgt +4    ; +4 if (own) y <= $3a
$d076  subq.w #1,d2 ; rts        ; -1 instead of e2d0's -9 (same net effect)
```

`$3a` = 58 is its own constant. It is not `disc::DISC_FAR_ROW_Y` (70,
`$a25a`, the disc's own row split) and not player 1's row split (14,
`$f838`) — three different thresholds at three different addresses, kept
separate in `ai.rs` rather than merged into one.

## Row 0: the escape (`$e0d8`)

```
$e0d8  cmpi.b #$15,$6d2e ; beq fail    ; player 2's own state excludes
$e0e0  cmpi.b #$16,$6d2e ; beq fail    ;   four values: $15/$16/$1d/$1e
$e0e8  cmpi.b #$1d,$6d2e ; beq fail
$e0f0  cmpi.b #$1e,$6d2e ; beq fail
$e0f8  move.w $6d30,d1 ; subi.w #9,d1 ; cell = own grid_cell - 9
$e100  bmi    fail
$e102  ...   move.w $759e+2(cell*8),d1 ; andi #$7f ; bne fail
                                        ; (own floor cell still has HP -> no
                                        ;  escape needed)
$e11e  lea    $1556,a2                 ; $1556[cell]: 8-bit mask, bit n set
                                        ;   = escape direction code n allowed
$e128  lea    $155e,a2 ; +cell*8       ; $155e[cell]: up to 7 codes in
                                        ;   priority order, $ff-terminated
loop:  move.b (a2)+,d1 ; cmp #$ff; beq fail
       btst.l d1,d2 ; beq loop         ; first code the mask allows wins
$e140  lea    $15fe,a2 ; +code*4
       move.w 2(a2),d2 ; move.w (a2),d1 ; -> (target_x, target_y)
$e150  moveq  #0,d0 ; rts              ; SUCCESS
```

The three raw tables, read from `discram.bin` and cross-checked against the
loop that consumes them:

```
$1556 (8 bytes, one per floor cell 0..7, bit n = direction n usable):
  37 7f ef ce 73 f7 fe ec

$155e (8 rows x 8 bytes, $ff-terminated priority lists):
  01 05 02 04 ff ff ff ff    05 01 06 00 ff ff ff ff
  02 05 06 00 04 03 ff ff    06 01 02 04 00 07 ff ff
  01 06 05 03 07 00 ff ff    05 02 01 07 03 04 ff ff
  02 06 01 07 ff ff ff ff    06 02 05 03 ff ff ff ff

$15fe (8 x (word,word), the center of each direction code's cell):
  (20,64) (60,64) (100,64) (140,64)
  (20,54) (60,54) (100,54) (140,54)
```

The `x` values (20, 60, 100, 140) are `COLUMN_WIDTH`-spaced (40) column
midpoints; the `y` values (64, 54) sit either side of the `$3a` (58) row
split — this is "the center of one of player 2's 8 floor cells," matching
`$1556`/`$155e`'s own 8-entry shape.

## Row 1: the avoid (`$e158`)

Same four-state exclusion, then its own 8-slot disc scan (not `$cea6`'s —
this rule does not use `a5` at all):

```
loop over discs[0..8]:
  $e17e  tst.b $10(a2) ; bpl next          ; must be simulated
  $e186  d0 = $6d24 + $6d40                ; $6d24 is the fixed constant 99
  $e18e  d1 = disc.world_y                 ;   ("player+4" -- read directly
  $e192  d1 < d0 ? next                    ;   from discram.bin: 99 at BOTH
  $e198  d0 += $6d42                       ;   $6ca4 and $6d24)
  $e19c  d1 > d0 ? next                    ; Y window: [99+box2, 99+box2+box3]
  $e1a2  d1 = disc.world_z
  $e1a6  d1 <= own world_y ? next          ; must still be in front
  $e1ae  d1 = own world_x - 8 + box0
  $e1b8  disc.world_x <= d1 ? next         ; X window:
  $e1c0  d1 += 8 + box1                    ;  (own_x-8+box0, own_x+box0+box1)
  $e1c6  disc.world_x >= d1 ? next
  ; box test passed for this disc:
  $e1ca  d1 = disc.vel_x (disc+6)
  $e1ce  bmi -> try RIGHT ($e080) then LEFT ($e0ae)
         else -> try LEFT ($e0ae) then RIGHT ($e080)
  both fail -> fall through to $e112 -- ROW 0's escape table, exactly as
               the ST falls through (not a separate lookup)
  either succeeds -> target = (that side's x, own world_y) ; SUCCESS
next disc
all 8 exhausted -> fail
```

`box0..box3` are `Player::hit_box` — a fed field (the animation engine
copies it from frame data this crate does not carry), so this row's box
test is exactly as measurable as any other comparison against a fixture's
own recorded state.

`$e0ae` (step LEFT) / `$e080` (step RIGHT) both call `$d062` with `(own
world_y, candidate_x - 8-or-8)` and succeed exactly when the resulting
floor cell's HP is nonzero — the same "is this cell still there" check as
`$e2d0`, at yet another address.

## Rows 2–5: `aim`-gated, and why 2–4 can never fire

Rows 2, 3 and 4's tests all open the same way:

```
$d4ea  cmpa.l #0,a5 ; beq fail      ; a5 (from $cea6) must be non-null
$d4f6  tst.b  $11(a5) ; beq fail    ; disc+$11 (the OWNER byte) must be NONZERO
```

Row 5's test opens with the *opposite* polarity:

```
$d6b4  cmpa.l #0,a5 ; beq fail
$d6bc  tst.b  $11(a5) ; bne fail    ; owner must be ZERO
```

`disc+$11` is `docs/state-schema.md`'s still-open discr-ovl.2: "every trace
reads 0 on every live slot — no trace has ever seen a disc change hands."
That is not a gap in three fixtures; it is presently true of *every* trace
this project has ever recorded. So **rows 2, 3 and 4 are provably dead code
against every trace this project can produce today** — not merely unfired
in these three, but structurally unreachable until something (a fourth
fixture, or a live Hatari capture of a caught/re-served disc) makes
`disc+$11` move. Row 5, needing the opposite, is not excluded this way —
and given how often the observed `$6da1` bytes are simple direction bits
with no fire bit (Part 10's "ten joystick bit patterns"), row 5 (priority
12, threshold 230 — a ~90% roll) is the most likely single source of most
of them. Its test (own `world_y` vs. the sensed disc's `world_z`, offset by
a literal 12, then a fixed reach of 30) is fully read above; its action
(`$d6da`) writes a plan step through `$e30a` too, indexed off `$15fe` by a
value (`d4`) this phase did not trace back to its source, so the row is not
implemented.

## The wall: `$6c5d` is not a fed field, and cannot become one after the fact

Eighteen of the twenty rows carry threshold < 255, so whether their reaction
roll passes depends on `$6c5d`. Four things about it, all address-cited
above: it free-runs (never reset by anything this phase found); it
increments for *every* row a frame reaches the roll for, not only the one
that ends up firing; which rows reach the roll depends on the current
latch, which is itself the *outcome* of past rolls; and it is shared with
at least three other call sites outside `$d2cc` entirely (`$da12` inside row
18's own test, and the retry loops the `dff6`/`d07a` chains use) — so its
value at any frame is coupled to a chain of decisions stretching back to a
reset this project has never observed.

That rules out reconstructing it after the fact, not just measuring it
directly: even a fixture that started at a *known* `$6c5d` could not be
replayed correctly without also replaying, correctly, priority-latch state
for all twenty rows across every frame back to that reset — which is
exactly the unimplemented eighteen rows. The dependency is circular by
construction, not merely hard. `$6ab5` (the roll's stride) is not itself a
mystery — it is `$6ab4`'s low byte, the same word-wide VBL frame counter
`lib.rs` already tracks, incrementing once a frame — but knowing the stride
does not help without the accumulator's starting point and its true update
count.

**Two rows sidestep this entirely**: threshold 255 makes their roll
unconditional (proved above), so rows 0 and 1 are exactly as measurable as
any other comparison this project makes, and no others are — not "not yet
transcribed," but presently outside what any fixture can check.

## Measured agreement

```
cargo test -p disc-core --lib ai::agreement -- --nocapture
```

| fixture | comparable ticks | agree | note |
|---|---|---|---|
| `golden.ndjson` | 99 | 18 | rows 0/1 never fire; 18 is exactly the fraction of ticks whose own `ai_6da1` is already 0 |
| `tile_damage.ndjson` | 214 | 61 | same shape |
| `p1_walk.ndjson` | 200 of 274 | 22 | see below |

"Comparable ticks" already excludes every transition with `updates != 1`
(Part 11f/11g): a 0-pass tick ran no `$d2cc` this crate could have
predicted, and a 2-pass tick ran it against an intermediate disc position
no fixture records, so both are skipped rather than guessed at — this is
why `p1_walk` measures 200 of its 274 clean ticks, matching Part 11f's own
"1 on 200, 2 on 37, 0 on 37" exactly.

`golden` and `tile_damage`: rows 0/1 never fire (no destroyed floor cell
under player 2, no disc in the avoid box), so `Ai::p2_policy` predicts 0
every tick, and the agreement number is exactly the fraction of ticks whose
own recorded byte happens to be 0 already — a real, if unflattering, number:
most of what these fixtures show `$6da1` doing comes from the eighteen
undecoded rows.

`p1_walk`: 174 of its 178 disagreements are the same shape. **Four are
not**, and are worth stating precisely rather than folding into the same
explanation. At frame 256 player 2 enters state 11 (knocked down, the
mirrored `$ca12` Part 11i already modelled) while a disc still sits in row
1's avoid box; this module latches row 1 and steers (`predicted = $04`).
The ST's own byte there is `$06`, not `$00` — so whatever is happening is
not silence either. Neither row 1's own exclusion list (`$15`/`$16`/`$1d`/
`$1e`) nor `$e30a`'s (`$0d`/`$0e`/`$18`/`$19`) names state 11, so nothing in
the code transcribed above says a knockdown should suppress or change row
1's output. Inventing a fifth exclusion state to make the number match is
exactly the house rule against moving a number by guessing semantics — so
this is left open, named for whichever bead picks up player 2's remaining
32-entry state table (`docs/disc-notes.md:1241`).

## Cashing in the waiver: not yet

The brief's cash-in condition — 100% on all three fixtures, then drive
player 2 from the policy instead of the fed byte — does not apply: rows 0
and 1 alone reach nowhere near 100%, by construction (the other eighteen
rows produce most of the nonzero bytes these fixtures show, and this phase
does not implement them, for the `$6c5d` reason above, not for lack of
transcription time). Feeding `$6da1` from `Ai::p2_policy` today would fail
`core-check` outright, in exactly the frames this table shows. discr-b6x
stays open, and the fixed points a next phase would need are, in order of
leverage:

1. **Transcribe rows 6–17** (`$dd68`/`$de8e`/`$de12`/`$ddd4`/`$ddc4`/`$da84`,
   paired with `$deea`/`$df58`) — twelve rows, six position tests, likely
   the single biggest source of the walk-direction bytes (`$04`/`$08`/`$06`)
   these fixtures show most often. Still blocked on `$6c5d` for *whether*
   they fire, but decoding *what they'd do* would at minimum turn "an RNG
   row produced this" into a named row for every remaining mismatch.
2. **A live Hatari `$6c5d`/`$6ab5` watch**, one scenario, to see whether the
   accumulator is in fact reset at a point this project already instruments
   (round start, `$aa50`; a serve; a state transition) — the argument above
   rules out reconstruction from the *existing* fixtures, not from a
   fixture built to capture it.
3. **A live disc-ownership capture** (discr-ovl.2) would settle whether rows
   2–4 are truly permanently dead or just dead against every fixture on
   disk today.

## Files

* `crates/disc-core/src/ai.rs` — `Ai::p2_policy`, rows 0/1, and the
  `ai::agreement` measurement (`cargo test -p disc-core --lib ai::agreement
  -- --nocapture` prints the three numbers above).
* `crates/disc-core/src/lib.rs` — `pub mod ai; pub use ai::Ai;`, mechanical.
* `crates/disc-core/Cargo.toml` — `serde`+`serde_json` as **dev**-dependencies
  only, so `ai::agreement`'s tests can read the committed fixtures directly
  without depending on `disc-tools::main` (owned elsewhere in this fleet).

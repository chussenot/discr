# Part 12 (discr-qqt): the `$6d8e`/`$6d10`/`$6d90` writer

**Bead**: discr-qqt — `$6d8e` (served disc's `dir_kind`) and its per-player
damage companions at `player+$70` (`$6d10` p1, `$6d90` p2) had no writer
anywhere in the analysed Ghidra image. Found: they are not written by code
Ghidra's static analysis ever reached at all — the writer lives in a
contiguous "round setup" routine (roughly `$cb00`-`$cea4`) and a parallel
routine at `$11512`-`$11540` that neither `xref` nor `scan` sees any
reference to, from anywhere. This was answerable only from a live Hatari
session, not from the Ghidra project alone.

## Method

Ghidra's static `xref`/`scan` on the analysed image confirm the bead's own
claim — zero write references to any of the three addresses, and zero
static references to the addresses the live writer turned out to live at
(`$cd7c`, `$11512`, `$ce64`): `scan cd7c`, `scan 11512`, `xref 6d96`,
`xref 6d18` all return 0 hits. The code is present in RAM (it visibly
executes) but outside whatever Ghidra's own CFG walk from its entry points
ever reached — likely because it is reached only through the mode-select /
round-init call chain, which the analysed snapshot's flow never covers.

So the whole finding is from a live Hatari change-watch across the full
boot -> menu -> character select -> mode select -> match window, using a
custom driver (not a `collect.py` scenario, because `run_scenario()`'s
`watch` step only starts running *after* `enter_match()` already has a live
match — too late to catch a write that happens during navigation/boot).
The driver (`scripts/collect.py`'s `Hatari` class, driven directly) arms
`b ($addr).w ! ($addr).w :trace :lock` on all three addresses *before*
`navigate_to_match()` even taps SPACE, keeps them armed through the full
nav, then holds Fire and waits inside the match. Three full fresh boots
were run this way, one per mode (challenge/training/tournament), each
~90s, `--protect-floppy` implied by `collect.py`'s `Hatari.start()`. Two
followups then targeted PC directly:

* wide `disasm` over the resident code once a match was live (reusing a
  cached savestate, no reboot needed — the code stays resident for the
  whole round even after these values are written, so a stale cache still
  shows it), to get instruction-level context around every hit;
* a second, *unconditional* PC breakpoint set (`b pc = $addr :trace :lock`,
  no value condition) on the five candidate writer instructions, to settle
  execution independent of whether the value written happened to match
  what was already there (a real gotcha, see below).

## The bracket: WHEN

Boot noise only, until the match is live:

| VBL | PC | what | note |
|---|---|---|---|
| ~975 | `$11f0` (`move.w (a1)+,d2`) | walks through `$6d8e`/`$6d90` as data | TOS/EmuTOS boot-time memory scan, sequential auto-increment read, not a writer |
| ~2420 | `$1028` (`movem.l d1-d4/d7/a1-a3,($20,a0)`) | writes `$6d10`=1, `$6d90`=0 incidentally | a wide register-block save whose target range happens to cover these addresses; this is what pre-loads `$6d10`=1 well before the real per-player writer runs (see the "silent write" gotcha below) |
| **~16300-18070** | **`$cda6`/`$cdaa`** (challenge, tournament) or **`$ce70`/`$ce76`** (training) | **the real writer** | fires once, automatically, ~100-200 VBLs after `navigate_to_match()` returns with the match already live — **before any served disc is thrown**, and identically whether or not the scenario ever presses Fire (training never serves the disc at all and still gets the write) |

Conclusion: **this is an unconditional per-round setup step, not a
serve-triggered one.** It runs once, early in the match/round's own
internal countdown, for both player slots, regardless of which mode was
selected or whether a disc is ever served in that mode.

## The mechanism: WHO, and its source data

Two parallel, near-identical "player-slot init" blocks exist, one per
player struct (base `$6ca0` = player 1's slot, base `$6d20` = player 2's
slot — confirmed independently by `docs/state-schema.md`'s own row 132,
"`player+$6e` / `+$70`": `$6ca0+$6e=$6d0e`, `$6ca0+$70=$6d10`,
`$6d20+$6e=$6d8e`, `$6d20+$70=$6d90`, exactly the four addresses in play).

### Player 2's slot (`$6d8e`/`$6d90`) — two alternative paths

```
0000cccc  cmp.w   #$0010,$00006c60.w
0000ccd2  bge.w   $0000cdb0                 ; $6c60 >= 16  ->  skip the table, use constants
...
0000cd7c  movea.l $00006d96.w,a0            ; a0 = selected character's record pointer
0000cd80  move.w  ($0008,a0),d0             ; d0 = that character's "level" stat
0000cd84  lea.l   $00011542,a0              ; a0 = the row table
[loop, 5 words/row: threshold,valA,valB,mag_raw,mag]
0000cd8a  move.w  (a0)+,d1                  ; d1 = row.threshold
0000cd8c  move.w  (a0)+,d2                  ; d2 = row.valA        -> $6d94
0000cd8e  move.w  (a0)+,d3                  ; d3 = row.valB        -> $6d32
0000cd90  move.w  (a0)+,d4                  ; d4 = row.mag_raw
0000cd92  neg.w   d4
0000cd94  move.w  (a0)+,d5                  ; d5 = row.mag
0000cd96  cmp.w   d1,d0
0000cd98  bge.w   $0000cd9e                 ; first row whose threshold <= d0 wins
0000cd9c  bra.b   $0000cd8a                 ; else advance to the next row
0000cd9e  move.w  d2,$00006d94.w
0000cda2  move.w  d3,$00006d32.w
0000cda6  move.w  d4,$00006d8e.w            ; <-- WRITER: dir_kind = -row.mag
0000cdaa  move.w  d5,$00006d90.w            ; <-- WRITER: damage   =  row.mag (same table field, unsigned)
0000cdae  rts
```

versus the fallback, reached only when `$6c60 >= 16`:

```
0000ce64  move.w  #$000c,$00006d94.w
0000ce6a  move.w  #$003c,$00006d32.w
0000ce70  move.w  #$ffff,$00006d8e.w        ; <-- WRITER: dir_kind = -1, hardcoded
0000ce76  move.w  #$0001,$00006d90.w        ; <-- WRITER: damage   =  1, hardcoded
0000ce7c  rts
```

### Player 1's slot (`$6d0e`/`$6d10`) — one path, always taken

```
00011512  movea.l $00006d18.w,a0            ; a0 = player 1's OWN character record pointer
00011516  move.w  ($0008,a0),d0
0001151a  lea.l   ($0026,pc),a0             ; = $00011542, the SAME table
[same 5-word row loop]
00011530  move.w  d2,$00006d16.w
00011534  move.w  d3,$00006cb2.w
00011538  move.w  d4,$00006d0e.w            ; <-- WRITER: player 1's own field, NOT negated
0001153c  move.w  d5,$00006d10.w            ; <-- WRITER: damage = row.mag
00011540  rts
```

Player 1's block has no `$6c60`-style gate and no constant-fallback twin —
confirmed by the unconditional PC breakpoint below, it runs identically in
every mode tested.

### The row table, at `$11542` (8 rows x 5 words = 80 bytes)

| row | threshold | valA | valB | mag_raw (`->6d8e`, negated) | mag (`->6d90`/`6d10`) |
|---|---|---|---|---|---|
| 0 | `$4000` (16384) | 15 | 26 | 3 | 3 |
| 1 | `$2000` (8192) | 12 | 24 | 3 | 2 |
| 2 | `$1000` (4096) | 10 | 22 | 2 | 2 |
| 3 | `$0800` (2048) | 9 | 20 | 2 | 2 |
| 4 | `$0400` (1024) | 8 | 18 | 2 | 1 |
| 5 | `$0200` (512) | 7 | 16 | 2 | 1 |
| 6 | `$0100` (256) | 6 | 14 | 1 | 1 |
| 7 | `$0000` (0) | 5 | 12 | 1 | 1 |

Rows are scanned from row 0 downward; the first row whose `threshold <=
d0` wins (`d0` = the selected character's own stat word at
*record*+`$8`). **This table is the direct, measured source of the
established fact "one per-player number is both throw depth-speed and
damage"**: columns 4 and 5 of a row hold the same magnitude, written twice
— once negated into the dir_kind field, once as-is into the damage field.

### The character records themselves

`$6d96` (p2's slot) and `$6d18` (p1's slot) are pointers to the *selected
character's* record in a roster array, 32 bytes/record, an 8-byte name
first:

```
p2's roster (as selected by default-fire nav, VBL 18018 capture):
  $77de  "EAGLE   "  ...  word@+8 = $4000   -> row 0 -> mag=3
  $77fe  "SHARK   "  ...  word@+8 = $6090   (next roster slot, not selected here)

p1's roster:
  $79be  "MACDO   "  ...  word@+8 = $0000   -> row 7 -> mag=1
  $79de  "YEYE    "  ...  word@+8 = ...     (next roster slot, not selected here)
```

Measured result for this capture: EAGLE (p2) -> `$6d8e=-3`/`$6d90=3`; MACDO
(p1) -> `$6d0e=1`/`$6d10=1` — both match the bead's own established
baseline exactly (`$6d8e=-3` idle reference, `$6d10=1`/`$6d90=3` Part 10c).
**This settles the "per-character or per-rank" question the bead raised:
it is per-character** — a stat word at a fixed offset in the selected
character's own roster record picks a row in a fixed table, and that row's
magnitude is what feeds both the dir_kind and the damage field. Both
players default to the same roster position in `navigate_to_match()`'s
own default-fire nav (leftmost chooser), so a genuine cross-character
comparison came for free from comparing p1 vs p2 rather than needing a
fourth boot with modified nav input.

### The mode gate: `$6c60`, set from `$6ca0`

`$6c60 >= 16` is what routes player 2's slot away from the character table
and into the hardcoded `-1`/`+1` fallback. `$6c60` is set from the mode
selector at four case sites (`$116b6`-`$1170e`), of which the one that
matters here:

```
000116b6  cmp.b  #$01,$00006ca0.w         ; mode byte == 1 (measured: TRAINING)
000116bc  bne.b  $000116d4
000116be  move.w #$0000,$00006c9e.w
000116c4  move.w #$0010,$00006c60.w      ; <-- sets the gate that skips the table
000116ca  lea.l  $000067f2.w,a0
000116ce  move.l a0,$00006d96.w
```

versus mode 2 (measured: CHALLENGE), which loads `$6d96` from the real
roster (`$77de`) and never touches `$6c60`.

## Confirming execution, not just value-change (a real gotcha)

The change-watch technique (`b (addr).w ! (addr).w`) never fired on
`$6d10`/`$6d0e` in *any* of the three full-boot captures — only the
boot-noise `movem.l` at VBL~2420 showed up. That looked, briefly, like
`$11538`/`$1153c` might be dead code. It is not: the `movem.l` at `$1028`
(a wide register-block save, unrelated to this feature) happens to leave
`$6d10=1` sitting in memory well before the real writer runs, and MACDO's
row (row 7, mag=1) writes that exact same value again — a value-preserving
write that a change-watch structurally cannot see. An *unconditional* PC
breakpoint (`b pc = $addr :trace :lock`, no value condition) settles it:

| PC | challenge | training |
|---|---|---|
| `$1153c` (p1 table writer) | 1 hit | 1 hit |
| `$cda6`/`$cdaa` (p2 table writer) | 1 hit each | 0 |
| `$ce70`/`$ce76` (p2 constant fallback) | 0 | 1 hit each |

Exactly as the `$6c60` branch model predicts: player 1's writer runs in
every mode; player 2's runs one path or the other, never both, gated on
`$6c60`.

## Variation matrix (measured)

| axis | result |
|---|---|
| challenge vs tournament (same default character both times) | identical: `$6d8e=-3`/`$6d90=3` at PC `$cda6`/`$cdaa`, VBL 18018 vs 18070 |
| challenge vs training | **different mechanism, not just different value**: table lookup (`$cda6`/`$cdaa`) vs hardcoded constant (`$ce70`/`$ce76`, always `-1`/`1` regardless of character) |
| p2 (EAGLE, stat `$4000`) vs p1 (MACDO, stat `$0`) in the same challenge match | different row (0 vs 7) -> different magnitude (3 vs 1) -- confirms per-character variation |
| championship | not run this pass (challenge + tournament + training covered; championship shares the same `$116xx` case-dispatch structure and was not separately booted for budget reasons) |

## Suggested patch (diff form; I own no core source files this round)

`docs/state-schema.md` row 132 can be sharpened from "nothing in the image
writes" to naming the mechanism — the fields stay waived (no Rust model of
character rosters/the row table exists), but the note is no longer
"unknown writer":

```diff
--- a/docs/state-schema.md
+++ b/docs/state-schema.md
@@ -129,7 +129,10 @@
 | `discs[n].aim` | `PlayerId` | `disc+$11` | waived:discr-ovl.2 |
 | `players[n].anim_cursor` | `u32` | `player+$3a` | waived:discr-75o |
-| `players[n].throw_dir_kind` / `throw_damage` | `i16` | `player+$6e` / `+$70` | waived:discr-qqt |
+| `players[n].throw_dir_kind` / `throw_damage` | `i16` | `player+$6e` / `+$70` | waived:discr-qqt (writer found Part 12: `$cda6`/`$cdaa` p2, `$11538`/`$1153c` p1 -- a per-character stat, `$11542`'s row table, no roster/table model in `disc-core`) |
```

No `disc-core` change is proposed: modelling this needs a character-roster
table and the row table, neither of which exists in the crate today, and
that's a bigger unit of work than this bead's remit (finding the writer).

## Files

- `reports/part12-dirkind.md` (this file)
- `docs/disc-notes.md` (appended section below)
- Evidence lives in this session's `tmp/watchboot_*.log`, `tmp/watchpc_*.log`,
  `tmp/wider_*.log`, `tmp/table_*.log`, `tmp/blocks_*.log` (gitignored,
  regeneratable from the scenario/driver described above; not committed).
- No `crates/` files touched (all owned by sibling agents this round per
  the lease board); the one suggested change is the `docs/state-schema.md`
  diff above, for the orchestrator to land.

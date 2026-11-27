# Part 14 (nsq) — `$1170` is not a continuation of the control stream; the real bug was two off-by-ones, and NSQ now decodes to pictures (discr-lul)

**Verdict: implemented.** The bead's premise was that `W == 0` branches to a
*separate sub-decoder reading a different stream* that can loop back into the
main reader, and that `a5`/`d1`'s setup had to be traced before it could be
implemented. Tracing `a5`/`d1` (below) shows something simpler and different:

- `$1170` reads **the row table that sits before the control stream** (`a5`),
  not more control-stream bytes. `W == 0` really *is* the end of the `a4`
  stream; the old `if w == 0: break` was right about that.
- `d1` is simply `$FFFF`, set at `$10ac`, and is the row table's own
  terminator — the very word the `$10b8` scan already finds.
- Nothing in `$1170`-`$125c` ever branches back to `$10c4`; `$111e` and
  `$116c` are the two block types' normal "next control word" jumps *inside*
  the main loop (Finding 4 of `reports/part13-codec.md` misread them).
- The three disagreeing NSQ numbers were not noise around a missing
  continuation. The decoder's **run lengths were one too long** (`ctrl+1`,
  `(ctrl & $7f)+1`; the code does `ctrl` and `ctrl & $7f`). That overshoots
  almost every RLE block, and the decoder only looked clean because it
  tolerated the overshoot and kept running until it met some later zero word —
  on a buffer that happened to continue into the next file, it always found one.

All listings below are from `assets/original/LAUNCHER.HA` (HABS, load `$1000`,
file offset = address − `$1000` + 28), disassembled with `capstone`
(`CS_ARCH_M68K`, big-endian; `md.skipdata = True`) — read as raw bytes, no
Ghidra. File: `scripts/loriciel_depack.py`, which now carries the port.

## 1. Where `a5`, `d1`, `d3`, `a4`, `a3`, `a2` come from

```
$1060  lea.l  $8(a0,d0.w), a5      ; a5 = &table entry          (see part13-formats.md §1)
$1072  lea.l  (a0,d0.l), a5        ; a5 = file base + entry offset
```

```
$1076  move.w   (a5), d0
$1078  ror.w    #$8, d0
$107a  move.w   d0, d1
$107c  andi.w   #$f800, d1
$1080  cmpi.w   #$f800, d1
$1084  bne.b    $108a
$1086  suba.l   a4, a4
$1088  bra.b    $109e
$108a  movem.l  d0-d7/a0-a6, -(a7)
$108e  lea.l    (a5), a2
$1090  lea.l    (a4), a3
$1092  bsr.w    $126a
$1096  movem.l  (a7)+, d0-d7/a0-a6
$109a  lea.l    $20(a5), a5
$109e  move.l   a4, -(a7)
```


`a5` starts at the entry. A record whose first word masks to `$f800` is not a
palette record (`a4 := 0`, `a5` untouched, `$1086`); otherwise `$126a` descrambles
the 32 bytes into `a4` and `a5 += 32` (`$109a`). `$10a0`-`$10a8`: the pushed
index (`d0`, post-wrap) `== 0` calls `$1004`, which zeroes `a2`'s 32000 bytes
(`$1020`: `d0 = $f9`, four `movem.l` of eight zero registers = 128 B per pass,
250 passes; `$1008` `lea (a2),a0`).

```
$10ac  move.w   #$ffff, d1              ; d1 := $FFFF, never written again up to $1268
$10b0  cmp.w    (a5), d1                ; a5 still at the row table start (record skipped)
$10b2  beq.w    $1260                   ; already $FFFF -> empty frame
$10b6  lea.l    (a5), a4                ; a4 := a5 ...
$10b8  cmp.w    (a4)+, d1               ; ... scan a4 forward past the first $FFFF word
$10ba  bne.b    $10b8
$10bc  lea.l    -$2000(a3), a3          ; a3 := bank base (a3 - $2000)
$10c0  move.w   #$0, d3                 ; d3 := 0
$10c4  move.b   $1(a4), d0              ; W = src[i+1]<<8 | src[i]  (a4 = control stream)
$10c8  lsl.w    #$8, d0
$10ca  move.b   (a4), d0
$10cc  lea.l    $2(a4), a4
$10d0  cmp.w    d3, d0                  ; W == 0 ?
$10d2  beq.w    $1170                   ; yes -> pass 2 at $1170
```


- `d1 = $FFFF` at `$10ac`. A search of `$10c4`-`$1268` for any use of `d1`
  finds exactly two: the `cmp.w d1,d0` at `$1172` and the register restore
  `movem.l (a7)+,d1-d7/a1-a6` at `$1264` on the way out (`$1004`'s zeroing of
  `d1` is before `$10ac`). So `$1172`'s "word read from `a5` equals `d1`" is "word equals
  `$FFFF`".
- `a5` is **not** advanced by the scan. `$10b6` copies it to `a4`; `$10b8`/`$10ba`
  advance only `a4`, to just past the first `$FFFF`. After the scan: `a5` = the
  row table's first word, `a4` = the control stream's first byte, and both
  `$FFFF`s are the *same* word.
- `d3 = 0` at `$10c0`; `$10d0`/`$10d2` is `W == d3` -> `$1170`.
- `$10bc`: `a3 -= $2000` — the tile bank base used by both passes.
- `a2`, `a3`, `a4` come from the caller. The caller (`$2c3e`-`$2e2e`, four call
  sites `$2cb4`, `$2d32`, `$2dc0`, `$2e2e`) loads `a0 = [$2e86]` (NSQ file
  base), `a1 = [$12a4]`, `a2 = [$12a0]` (screen), `a3 = [$12a8]` (bank), `a4 =
  $3454` (palette buffer), and after each call swaps `[$12a0]`/`[$12a4]`
  (`$2cc8`-`$2cd8`) and copies 32 bytes at `[$2e8a]` to `$ff8240`:

```
$2c72  lea.l    $ff8260.l, a1
$2c78  movea.l  $2e8a.l, a0
$2c7e  movem.l  (a0)+, d0-d7
$2c82  movem.l  d0-d7, -(a1)
$2c86  movem.l  (a7)+, d0-d7/a0-a6
```


  `[$2e8a]` is only updated when `$1260` hands back a nonzero `a0` (`$2cb8`-
  `$2cc2`), i.e. when the frame had a palette record (`$109e` pushes `a4`,
  `$1260` pops it into `a0`).

## 2. Pass 1 — `$10c4`-`$116c`, the tile-bank stream (`a4`)

```
$10d6  move.w   d0, d2
$10d8  andi.w   #$f, d2                 ; d2 = nib
$10dc  eor.w    d2, d0                  ; clear nib in W
$10de  bclr.b   #$f, d0                 ; test+clear bit 15 (RLE flag); Z set = raw
$10e2  beq.b    $1120                   ; raw -> $1120
$10e4  ext.l    d0
$10e6  add.l    d0, d0                  ; byte offset (W&$7ff0)*2 = tile*32
$10e8  lea.l    (a3, d0.l), a0          ; a0 = bank + tile*32
$10ec  addq.w   #$1, d2                 ; nib+1
$10ee  mulu.w   #$10, d2                ; d2 = (nib+1)*16  WORDS to produce
$10f2  moveq    #$0, d0
$10f4  move.b   (a4)+, d0               ; ctrl byte
$10f6  bclr.b   #$7, d0                 ; test+clear bit 7
$10fa  bne.b    $110e                   ; bit 7 set -> literal
$10fc  sub.w    d0, d2                  ; FILL: d2 -= ctrl
$10fe  subq.w   #$1, d0                 ; d0 = ctrl-1
$1100  move.b   (a4)+, d4
$1102  lsl.w    #$8, d4
$1104  move.b   (a4)+, d4
$1106  move.w   d4, (a0)+               ; write the fill word ...
$1108  dbra     d0, $1106               ; ... ctrl times in all (dbra)
$110c  bra.b    $111a
$110e  sub.w    d0, d2                  ; LITERAL: d2 -= (ctrl&$7f)
$1110  subq.w   #$1, d0                 ; d0 = n-1
$1112  move.b   (a4)+, (a0)+
$1114  move.b   (a4)+, (a0)+
$1116  dbra     d0, $1112               ; n words copied
$111a  tst.w    d2                      ; budget exactly spent?
$111c  bne.b    $10f2                   ; no -> next run
$111e  bra.b    $10c4                   ; yes -> next control word
$1120  ext.l    d0
```


Reading it:

| field | code | meaning |
|---|---|---|
| `W` | `$10c4`-`$10ca` | `src[i+1]<<8 | src[i]` |
| `W == 0` | `$10d0`/`$10d2` | end of this stream -> `$1170` |
| `nib` | `$10d8` | `W & 15` |
| tile | `$10dc`-`$10e8` | `(W & $7ff0) * 2` bytes into the bank = tile `(W & $7ff0) >> 4`, 32-byte tiles |
| raw (`W & $8000 == 0`) | `$1120`-`$1168` | `(nib+1)` passes of 32 byte-moves; `dbra d2` with `d2 = nib` |
| RLE | `$10ec`/`$10ee` | `d2 = (nib+1) * 16` **words** to produce |
| fill (`ctrl & $80 == 0`) | `$10fc`-`$1108` | `d2 -= ctrl`; `d0 = ctrl-1`; one source word written by a `dbra` loop that runs `ctrl` times |
| literal (`ctrl & $80 != 0`) | `$110e`-`$1116` | `d2 -= ctrl&$7f`; `ctrl&$7f` words copied |
| budget | `$111a`/`$111c` | `tst.w d2 / bne.b $10f2`: runs continue until `d2 == 0` **exactly** |

Neither run form has a `+1`. `ctrl == 0` would make `dbra` wrap 65536 times, and a
run longer than the remaining `d2` can never reach zero, so neither occurs in
valid data: measured, zero-length runs `0/9311`, longest run `126` words.

The pre-discr-lul decoder (`scripts/loriciel_depack.py` as of `HEAD~`) used
`reps = (ctrl & $7f) + 1` and `reps = ctrl + 1` and `while count > 0`.

## 3. Pass 2 — `$1170`-`$125c`, the blit list (`a5`)

```
$1170  move.w   (a5)+, d0               ; PASS 2: word from a5
$1172  cmp.w    d1, d0                  ; == d1 ($FFFF)?
$1174  beq.w    $1260                   ; yes -> return
$1178  ror.w    #$8, d0                 ; byte swap
$117a  move.w   d0, d2
$117c  andi.w   #$f800, d2
$1180  cmpi.w   #$f800, d2
$1184  bne.b    $11b0                   ; not a position word -> descriptor already in d0
$1186  eor.w    d2, d0                  ; cell = w ^ $f800
$1188  move.w   d0, d4
$118a  ext.l    d4
$118c  divu.w   #$28, d4                ; row = cell / 40, d4.high = col
$1190  move.w   d4, d2
$1192  swap     d4
$1194  move.w   d4, d7
$1196  mulu.w   #$500, d2               ; row * $500
$119a  bclr.b   #$0, d4                 ; col odd? (Z clear) ...
$119e  beq.b    $11a2
$11a0  addq.w   #$1, d2                 ; ... +1 byte
$11a2  add.w    d4, d4                  ; (col&~1)*4
$11a4  add.w    d4, d4
$11a6  add.w    d2, d4
$11a8  lea.l    (a2, d4.w), a0          ; a0 = screen + offset
$11ac  move.w   (a5)+, d0               ; next word = the record
$11ae  ror.w    #$8, d0
$11b0  move.w   d0, d5
$11b2  andi.w   #$7800, d5              ; reps field
$11b6  move.w   d0, d6
$11b8  andi.w   #$8000, d6              ; run flag
$11bc  andi.w   #$7ff, d0               ; tile index
$11c0  move.w   d0, d2
$11c2  ext.l    d2
$11c4  lsl.l    #$5, d2                 ; tile*32
$11c6  lea.l    (a3, d2.l), a4          ; a4 = bank + tile*32
```


```
$122c  addq.w   #$1, d7                 ; col++
$122e  cmpi.w   #$28, d7
$1232  bcs.b    $123c
$1234  lea.l    $467(a0), a0            ; wrap: next cell row
$1238  moveq    #$0, d7
$123a  bra.b    $124a
$123c  btst.b   #$0, d7
$1240  bne.b    $1246
$1242  lea.l    $6(a0), a0              ; even col: +6
$1246  lea.l    $1(a0), a0              ; +1
$124a  tst.w    d6                      ; run flag set?
$124c  beq.b    $1254
$124e  addq.w   #$1, d0                 ; tile++
$1250  lea.l    $20(a4), a4             ; a4 += 32
$1254  subi.w   #$800, d5
$1258  bcc.w    $11ca                   ; more reps -> $11ca
$125c  bra.w    $1170                   ; next record
$1260  movea.l  (a7)+, a0               ; return
$1262  move.w   (a7)+, d0
$1264  movem.l  (a7)+, d1-d7/a1-a6
$1268  rts      
```


- `$1170`-`$1174`: a word (compared raw) equal to `d1` (`$FFFF`) returns via `$1260`.
- `$1178`: byte swap. `$117c`-`$1184`: `(w & $f800) == $f800` is a **position** word.
  `$1186`: `cell = w ^ $f800`. `$118c` `divu.w #$28`: `row, col = divmod(cell, 40)`;
  `$1196`: `row * $500` (8 pixel rows of 160 bytes); `$119a`-`$11a8`: `(col & ~1) * 4 +
  (col & 1)` — the 16-pixel group's byte pair. `$11ac` reads the next word as the
  record. Any other word is itself the record, with `a0`/`d7` carried over from
  the previous record (`$123c`-`$1246` step one cell at a time; `$1234` wraps a row).
- Record (`$11b0`-`$11c6`): `reps = ((w & $7800) >> 11) + 1` (`$1254`/`$1258`
  `subi.w #$800,d5 / bcc.w $11ca` loops `reps` times), run flag `w & $8000`
  (`$124e`/`$1250`: `d0 += 1`, `a4 += 32` per cell), `tile = w & $7ff`
  (`$11bc`, `lsl.l #5` `$11c4`). The tile is read from the **same** bank pass 1
  fills.
- Cell blit (`$11cc`-`$1228`): 8 rows; row `r` loads bank word `[r]` into the
  high half and word `[8+r]` into the low half of `d2`, `movep.l d2,(r*160)(a0)`
  — one byte per plane, stride 2.
- `$125c bra.w $1170` is the loop; `$1260`-`$1268` the only exit.

Nothing in `$1170`-`$1268` branches to `$10c4`. Every branch in that range
(`$1174`, `$1184`, `$119e`, `$1232`, `$123a`, `$1240`, `$124c`, `$1258`, `$125c`)
targets `$1260`, `$11b0`, `$11a2`, `$123c`, `$124a`, `$1246`, `$1254`, `$11ca` or
`$1170`. The only direct branches to `$10c4` in the whole linear disassembly are `$111e` and `$116c`, inside pass 1,
and `$10d2` (plus the loop `$125c`) are the only direct branches to `$1170`.

## 4. Palette record (`$126a`)

```
$126a  movem.l  d0-d7/a0-a6, -(a7)
$126e  moveq    #$f, d7
$1270  move.b   (a2)+, d0
$1272  move.b   d0, d1
$1274  move.b   (a2)+, d2
$1276  lsr.b    #$1, d2
$1278  roxr.b   #$2, d1
$127a  andi.b   #$70, d1
$127e  move.b   d0, d2
$1280  andi.b   #$7, d2
$1284  or.b     d2, d1
$1286  lsr.b    #$3, d0
$1288  andi.b   #$7, d0
$128c  move.b   d0, (a3)+
$128e  move.b   d1, (a3)+
$1290  dbra     d7, $1270
$1294  movem.l  (a7)+, d0-d7/a0-a6
$1298  rts      
```


Per input byte pair `(in0, in1)`: `out0 = (in0 >> 3) & 7`; `out1 = (rox_right2(X=in1&1,
in0) & $70) | (in0 & 7)`. 16 pairs -> 16 words `0RGB`, 3 bits per gun. The caller
then copies exactly these 32 bytes to `$ff8240` (section 1), which settles
that it is a palette.

## 5. Measurements

All on the same image, `assets/disch/DSC`; the two `.NSQ` files at the directory's
own offsets `$50c00` (`50.NSQ`, 69428 B, 50 frames) and `$61c00` (`LORI.NSQ`,
25592 B, 36 frames); both are byte-identical to `assets/original/*.NSQ`.

### Before (the unmodified `decode_nsq_control_stream()`, same helper functions)

| method | what "accounted" means | 50.NSQ | LORI.NSQ |
|---|---|---|---|
| A — read from the file's start to the **end of the disk image** | decoder returned without raising (no check that it met `W == 0`) | 50/50 | 36/36 |
| B — file bytes **only** (`assets/original/*.NSQ`, or the image capped at `byte_size`) | same | 38/50 | 32/36 |
| C — same as A but count only streams whose old-semantics run reaches `W == 0` with every RLE budget landing on exactly zero | | 3/50 | 11/36 (2 + 9 empty) |

The third prior figure (`34/50 + 30/36`) did **not** reproduce under any
variant tried (A, B, A-with-end-within-`byte_size` = 38/50 + 32/36, C). The
earlier "31/36" for B reproduces as 32/36. A and B differ only in whether the
buffer continues past the file; B's 12+4 failures are exactly the frames the
old decoder overshot past the end, and A's "success" on them is the decoder
wandering into `LORI.NSQ` and the gap between the files.

### After

`python3 scripts/loriciel_depack.py --nsq-check assets/disch/DSC` (each file
read as **exactly** its `byte_size`):

```
50.NSQ     frames=50 empty=0 accounted=50/50
LORI.NSQ   frames=36 empty=9 accounted=36/36
```

A frame is accounted only if **all** of these hold (see `nsq_check()`):

1. pass 1 reaches `W == 0` inside the file, every RLE block spending its
   `(nib+1)*16`-word budget to exactly zero — or the frame is empty;
2. pass 2's record parse ends on the very `$FFFF` the `$10b8` scan found;
3. every tile pass 2 blits was written by pass 1 of this or an earlier frame;
4. the frame's data ends — rounded up to an even address, since the 68000
   reads the table with word accesses — exactly where the next entry (sorted
   by offset) begins.

Measured pieces, so the number does not rest on the checker alone:

| claim | 50.NSQ | LORI.NSQ |
|---|---|---|
| asm run lengths (`ctrl`, `ctrl&$7f`): streams ending at `W == 0`, every budget exactly zero | 50/50 | 27/27 (+9 empty) |
| old lengths (`+1`/`+1`): same test | 3/50 | 2/27 |
| mixed (`fill+1`, `lit+0`) | 4/50 | 3/27 |
| mixed (`fill+0`, `lit+1`) | 3/50 | 1/27 |
| pass-2 parse ends on the same `$FFFF` | 50/50 | 27/27 |
| frame end (even-rounded) == next entry offset: exact / needs the 1-byte even pad | 32 / 17 | 17 / 18 |
| last frame ends exactly at `byte_size` | `$10f34` = 69428 | `$63f8` = 25592 |
| blits / blits of never-written tiles | 4250 / 0 | 2332 / 0 |
| max cell | 826 (< 1000) | 398 |

Item 4 is the independent proof: 86 frames' stream lengths tile each file with
no gap and no overlap, and the final frame of each ends on the byte. A
decoder with wrong run lengths cannot do that.

Negative controls: 200 single-bit flips in `50.NSQ`'s data region (after the header) give
`accounted` min 0 / mean 49.02 / max 50 (flips inside raw tile bytes are not
structural and correctly pass); truncating the file by 2 bytes fails the last
frame (`ran off the end (no W==0 terminator)`).

Under method A and B the new decoder agrees: both give 50/50 and 27 + 9 empty
(`decode_nsq_control_stream()` on `data[cs:]`, either buffer). **The three
disagreeing figures converge on one: 50/50 + 36/36**, and this time with a
definition that fails when the decoder is wrong. That the number equals the
very first figure is coincidence of a permissive check, not vindication of the
old decoder.

### Pictures

`nsq_play()` (one persistent bank and screen, frame order) rendered through the
descrambled palette is recognisable: `50.NSQ` frame 0 is the DISC title over the
mountain arena with three standing fighters (1000 cells, the full screen);
frames 1.. are 20-80-cell deltas that move the fighters; `LORI.NSQ` shows the
Loriciel panther logo sweeping in and "PRESENTS" below it (frame 0 is again a
full 1000-cell clear/draw; frames 1-3 and 30-35 are empty hold frames).
Reproduce: `loriciel_depack.py --nsq-ppm assets/original/50.NSQ 10 out.ppm`.
This is a strong test of the tile layout, the `movep` plane order, the cell
geometry and the palette descramble together: any of them wrong would give noise.

### `depack_ice()` round trip (must not change)

Run before and after the change, `loriciel_depack.py --extract assets/disch/DSC <NAME> out`:

| file | bytes | sha256 (before) | after |
|---|---|---|---|
| DALLES01.DAT | 31664 | `99a876e8...7d036` | identical |
| PLAYER01.DAT | 152360 | `b39c713d...bc2c` | identical |
| ENEMY01.DAT | 63070 | `e3785640...01799` | identical |
| DECOR00.DAT | 32032 | `39414af0...9c2` | identical |

4/4 bit-exact before and after, matching `reports/part13-codec.md`'s table and
`cmp`-identical outputs.

## 6. Proven, not proven

**Proven** (address + measurement): pass 1/pass 2 structure; `d1 = $FFFF`;
`a5` = row table start; `W == 0` terminates the `a4` stream; both run-length
semantics (exact-zero budget, stream tiling of the whole file); the blit
geometry and tile layout; the palette descramble and that it feeds `$ff8240`.

**Not established**

1. **The caller's two-screen swap.** After every `$1048` the caller exchanges
   `[$12a0]`/`[$12a4]` (`$2cc8`-`$2cd8`, `$2d12`...) and the next frame is drawn
   into the *other* screen. Frame 1 has only 70 cells while frame 0 has 1000, so
   either the second screen already holds frame 0, or deltas are written
   against frame k-2. `nsq_play()` uses ONE screen. In a model that alternates two
   screens (second screen blank at start) even-indexed frames differ from the
   one-screen render in only 39-59 bytes (frames 10 and 12) and odd frames in
   ~10,700-10,900 bytes (9 and 11) — consistent with a blank second buffer, not
   settled by static reading. Next measurement: breakpoint at `$1048`, log
   `a2`, `[$12a0]`, `[$12a4]` and a hash of the 32000 bytes at `a2` on entry for
   the first ~6 calls (Hatari, `scripts/collect.py`).
2. `$11bc`'s record words with the top five bits all set (`$f800` pattern) are
   read as positions by `$1180`; a descriptor with run flag and `reps >= 15`
   would be indistinguishable, so such a record cannot exist in valid data.
   None occurs (the max `reps` field in the data is 14).
3. Whether other consumers call `$1048` with a different `a2`/`a3`: the four
   call sites in `$2c3e`-`$2e2e` are the only `bsr.w $1048` in the file.
4. The earlier shifts' `34/50 + 30/36` was not reproduced.

## 7. Files changed

- `scripts/loriciel_depack.py` — `decode_nsq_control_stream()` run lengths
  corrected and made strict; new `nsq_blit_list`, `nsq_frame_layout`,
  `nsq_decode_frame`, `nsq_play`, `nsq_descramble_palette`, `nsq_screen_to_rgb`,
  `nsq_check`, CLI `--nsq-check` / `--nsq-ppm`; header comment rewritten.
- `docs/loriciel-formats.md` — the `*.NSQ` section: the earlier "past the end of
  the file" explanation retracted, the real structure stated.
- `docs/disc-notes.md` — one appended section.
- this report.

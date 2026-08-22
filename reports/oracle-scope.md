# disc-oracle -- Phase 0 scope report

What the game actually needs from an Atari ST, measured on a live CHALLENGE
round in Hatari 2.6.1. Everything here is a measurement, not an assumption;
the probe that produced each number is named.

**Verdict: in scope.** The game touches 18 hardware addresses plus the IKBD
ACIA, executes no ROM, takes no traps, and never touches the FDC mid-match.
Two of its three interrupt sources write no RAM at all.

## 1. The sampling-point contract

    state(N) = memory as it stands AT ENTRY to the VBL handler for frame N,
               i.e. with PC == $8198 and BEFORE that instruction executes.

`$70` (vector 28, level-4 autovector) holds **`$00008198`**, and the handler
begins:

```
00008198  5278 6ab4     addq.w #$01,$00006ab4.w    <-- sample here, before this
0000819c  5378 6ab6     subq.w #$01,$00006ab6.w
000081a0  4a38 6c4a     tst.b  $00006c4a.w
```

So at the sampling point `$6ab4` still holds the *previous* frame's count.
Both sides must use `pc = $8198`.

**The existing `frame_trace()` did not quite honour this.** It breaks on
`b VBL ! VBL`, which usually lands on `$8198` but not always -- one hit in a
measured run of eight came from `$83fc`, inside the Timer A handler, because
Hatari's `VBL` variable ticks at the top of the frame and whatever instruction
runs next is what gets reported. Changed to `a $8198` (an address breakpoint,
i.e. `pc = $8198`), which fired exactly once per frame over 21 frames with
`HBL=0` every time. Machine state at that point:

    SR = $2404   (supervisor, interrupt mask 4)
    ISP = $7e20  USP = climbing ~99/frame (see Timer A below)

## 2. ROM, traps, and memory above 1 MB

`b pc > $dfffff :trace :lock` over ~40 in-match frames: **0 hits**. The same
for `pc > $fffff && pc < $e00000`: **0 hits**. A trap would vector into ROM
and so would have been caught by the first breakpoint, so there are no
GEMDOS/XBIOS calls in a match either.

The unused exception vectors do point at ROM (`$64`/`$68`/`$6c` ->
`$e00d68`/`$e00858`, MFP vectors `$100`-`$110` -> `$e007f0`), but nothing
raises those levels.

**Consequence: the seed is 1 MB of RAM and nothing else. No TOS image is
needed by the oracle.**

## 3. Hardware registers touched, ranked

30 in-match frames under `trace io_read,io_write`. 14842 accesses over
**18 distinct addresses**:

| address | what | access | count | per frame | PCs |
|---|---|---|---|---|---|
| `$ffff8800` | PSG register select | write.b | 4662 | 155.4 | $83f0 $83f4 |
| `$ffff8802` | PSG data write | write.b | 4662 | 155.4 | $83f0 $83f4 |
| `$ffff8804` | PSG select (mirror) | write.b | 2331 | 77.7 | $83f0 |
| `$ffff8806` | PSG data (mirror) | write.b | 2331 | 77.7 | $83f0 |
| `$fffffa1b` | MFP TBCR (Timer B control) | write.b | 126 | 4.2 | $81ac $834c $8356 $8362 |
| `$ffff825e` | palette 15 | write.w | 95 | 3.2 | $81d4 $833e $8366 |
| `$fffffa1b` | MFP TBCR (Timer B control) | read.b | 63 | 2.1 | $834c $8362 |
| `$fffffa21` | MFP TBDR (Timer B data) | write.b | 63 | 2.1 | $81a6 $8350 |
| `$ffff8248` | palette 4 | write.w | 63 | 2.1 | $81c0 $832e |
| `$ffff824a` | palette 5 | write.w | 63 | 2.1 | $81c0 $832e |
| `$ffff824c` | palette 6 | write.w | 63 | 2.1 | $81c6 $8330 |
| `$ffff824e` | palette 7 | write.w | 63 | 2.1 | $81c6 $8330 |
| `$ffff8258` | palette 12 | write.w | 63 | 2.1 | $81ce $8332 |
| `$ffff825a` | palette 13 | write.w | 63 | 2.1 | $81ce $8332 |
| `$ffff825c` | palette 14 | write.w | 63 | 2.1 | $81d4 $8338 |
| `$ffff8201` | screen base high | write.b | 32 | 1.1 | $9712 |
| `$ffff8203` | screen base mid | write.b | 32 | 1.1 | $9712 |
| `$fffffa19` | MFP TACR (Timer A control) | write.b | 2 | 0.1 | $83fe $a6f0 |
| `$fffffa19` | MFP TACR (Timer A control) | read.b | 1 | 0.0 | $83fe |
| `$fffffa1f` | MFP TADR (Timer A data) | write.b | 1 | 0.0 | $a6ea |

Not present, and this matters: **no FDC** (`$FF8604`/`$FF8606`), no
`$FF8260` (resolution), no `$FF8240`-`$FF8246`, no disk or DMA registers.
The original re-reads the floppy constantly *between* rounds, but not during
one.

**The IKBD ACIA (`$FFFC00`/`$FFFC02`) is missing from this table only because
Hatari traces it under `ikbd_acia`, not `io_read`.** Part 5 established the
game polls `$FFFC02` directly at PC `$8372`/`$83b2`/`$83c2`. It belongs in the
stub list.

## 4. Interrupts

Measured by putting an address breakpoint on each handler and counting hits
per frame over ~21 frames:

| source | level | vector | handler | rate | writes below $8000? |
|---|---|---|---|---|---|
| VBL | 4 (autovector) | `$70` | `$8198` | **1.00 / frame** | **yes -- this is the game** |
| MFP Timer A | 6 | `$134` | `$83d2` | **92.7 / frame** | **no** |
| MFP Timer B | 6 | `$120` | `$8362` (re-pointed) | **1.00 / frame** | **no** |
| MFP ACIA / IKBD | 6 | `$118` | `$8370` | on packet arrival only | yes (input decode) |

MFP enable/mask registers read live:

    IERA $21  IMRA $21   -> Timer A (bit 5) and Timer B (bit 0) enabled
    IERB $40  IMRB $40   -> ACIA (bit 6) enabled; Timer C and Timer D OFF
    TACR $01  TADR $7c   -> Timer A, delay mode, /4 prescale, count 124
    TBCR $08  TBDR $55   -> Timer B, event-count (HBL) mode

Timer C being disabled means EmuTOS's 200 Hz tick is not running. HBL (level
2) is never taken -- `SR` mask is 4 inside the VBL handler and the level-2
vector points at ROM, which we proved never executes.

### Why the two timers are cheap

**Timer A** is a 12-instruction PSG streamer. It reads its next byte through
**USP** used as a stream cursor, indexes a table at `$8410`, and `movep`s a
long and a word into `$FF8800`:

```
000083d2  movem.l d0-d1/a0,-(a7)
000083d6  move.l  usp,a0
000083da  move.b  (a0)+,d0          ; next byte of the tune
000083dc  beq.b   $83fe             ; 0 = end
000083de  move.l  a0,usp
000083e0  lsl.w   #$03,d0
000083e2  lea     ($002c,pc),a0     ; -> $8410
000083e8  move.l  (a0)+,d0
000083ea  move.w  (a0)+,d1
000083ec  movea.w #$8800,a0
000083f0  movep.l d0,($0000,a0)     ; PSG
000083f4  movep.w d1,($0000,a0)
000083fc  rte
```

**Timer B** is three instructions -- stop itself, write one palette entry:

```
00008362  clr.b   $fffffa1b.w
00008366  move.w  $0001571a,$ffff825e.w
0000836e  rte
```

Neither *steady-state* path writes a byte below `$8000`. Timer A's other
visible effect is that it advances USP (~99 per frame, consistent with 92.7
interrupts plus the end-of-stream path).

> **Correction, from Phase 3.** The paragraph that used to sit here concluded
> "a first oracle can omit both timers". That was wrong about Timer A, and the
> differ caught it within two bytes. The listing above is Timer A's *loop*; its
> **exit** path, which I had not disassembled, is:
>
> ```
> 000083fe  clr.b $fffffa19.w     ; stop the timer
> 00008402  clr.b $00006c5b.w     ; <-- below $8000
> 00008406  clr.b $00006c5c.w     ; <-- below $8000
> ```
>
> `$6c5b`/`$6c5c` are the "sound effect busy" latch that the disc engine sets
> at `$a6c4`-`$a6f0` before pointing USP at a sample and starting Timer A. With
> Timer A omitted the stream never terminates and the latch never clears, which
> is exactly the disagreement the differ reported. **Timer A is emulated for
> real in the oracle.** Timer B's claim stands: both its handlers (`$8320`, to
> which the VBL handler re-points vector `$120` every frame, and `$8362`) write
> only palette and MFP registers.
>
> The lesson is narrow and worth keeping: disassembling the hot path of a
> handler is not the same as disassembling the handler.

## 5. Consequences for the harness

- Seed: 1 MB RAM + registers. No ROM.
- Interrupts to implement first: level 4 at `$8198`, once per frame. Then the
  ACIA when the script injects input.
- Stubs: PSG (`$FF8800`-`$FF8806`) write-only, discard. Palette
  (`$FF8248`-`$FF825E`) and screen base (`$FF8201`/`$FF8203`) write-only,
  record but discard. MFP `$FFFA19`/`$FFFA1B`/`$FFFA1F`/`$FFFA21` need real
  read-back (the VBL handler writes them and Timer B reads TBCR).
- Everything else in `$FF8000`-`$FFFFFF` aborts the run.
- IKBD sends a joystick packet **on state change only** -- holding a
  direction produced just 2 ACIA interrupts, not one per frame. A stub that
  streams a packet every frame would be wrong.

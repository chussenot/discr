# bonus.ndjson -- provenance

152 frames. The fixture the project lacked since Part 10: one in which a
bonus is actually placed on a tile (bit 7 of its hp word) and picked up,
so `bonus_6d9a` is finally non-zero somewhere instead of 0 on every row of
every fixture that came before it.

    ./oracle/disc-oracle --seed tmp/bonus_diff.seed \
        --script tmp/idle.script --frames 152 --trace tests/fixtures/bonus.ndjson

* **Seed**: `tmp/bonus_diff.seed` (not committed -- gitignored like every
  other seed; MANIFEST-equivalent notes live here instead), sha256
  `723ffb52e01cee08...`, captured at PC == `$8198` in a live CHALLENGE round
  (`$6ab4` = 6411). Minted by polling `$6e3a` idle from
  `tmp/match_challenge.sav` (the same cache `scenarios/oracle_seed.yaml`
  uses) until it went non-zero, then seeding immediately in the SAME Hatari
  session -- so the seed and the Hatari reference trace below come from one
  continuous run, not two. The capture is reproducible byte-for-byte: two
  independent runs of the same recipe (enter the cached match, settle 30,
  poll `$6e3a` every 2 frames idle) produced the identical sha256, because
  the machine is deterministic from a savestate and idle input carries no
  wall-clock jitter into the emulated state.
* **Input**: none. Idle, like `tile_damage.ndjson` -- the bonus is placed and
  picked up by the ST's own timer-driven logic and the opponent's own play;
  no input program was needed to provoke either.
* **Why the window is trustworthy**: `scripts/oracle_diff.py` against a
  Hatari reference captured in the same session (`tmp/hatari_ref_bonus.json`)
  reports **147 frames of tier-1 (frame-exact) agreement** (Hatari-aligned
  frames 0-146, i.e. oracle frames 5-151 -- Hatari's trace starts 5 frames
  after the seed instant because minting the seed costs a `savebin`, same
  offset `tile_damage.provenance.md` already documents). The divergence at
  the next frame is in `$6c5d` (the PRNG byte this same bonus-roll advances)
  and the fields it feeds, not in anything this fixture is cited for -- see
  "The oracle-vs-Hatari divergence" below. **The bonus placement and pickup
  (oracle frames 145-151) sit entirely inside the validated window.**

## The bonus event

| oracle frame | event |
|---|---|
| 0-150 | `tiles[7].hp` = 132 (`0x84` -- bit 7 set, base hp 4), `bonus_6d9a` = 0 |
| 151 | `tiles[7].hp` -> 4 (bit 7 stripped), `bonus_6d9a` -> **2** |
| 151-159 | `bonus_6d9a` stays 2 (code 2's table row carries no timer countdown visible this soon; its duration is ~500 frames per Part 10's `$9aa2` table) |

Cell 7 of the near bank (`$7616`, `tiles[7]`, the bank `disc-core` already
models) is where this particular roll landed; nothing about the mechanism
below is cell-specific.

## discr-ovl.4: the placer, named

Static analysis (`tmp/ghidra_proj`, Ghidra 12.1.3) had exhausted itself
first: every literal reference to `$7616`/`$7596` anywhere in the 1MB RAM
image (38 total, verified against the raw binary bytes, not just Ghidra's
instruction model) is a known read (walkability test, or the `$a292`/`$a29c`
pickup test-and-strip) -- no static writer of bit 7 or of `$6e3a` exists
in that snapshot; the placer's code sits in a span Ghidra's batch analysis
never reached from a known entry point. A live change-watch on `$6e3a`
(Hatari's own change-tracking breakpoint, not Ghidra) found it in one
CHALLENGE-mode idle run:

```
$9d0c  cmp.b  #1,$6ca0        ; special/test path if $6ca0==1 (sets $6e3a=$0a
                              ; directly, gated on $6c9e==0 -- not the normal
                              ; mint, not exercised by this fixture)
$9d16  subq.w #1,$6e3c        ; $6e3c: the roll-gate COUNTDOWN. Reloaded to
$9d1e  move.w #$14,$6e3c      ; $14=20 each time it fires -- a bonus is only
                              ; ever ROLLED once every 20 ticks of whatever
                              ; calls this (measured: this routine sits beside
                              ; per-VBL bookkeeping for the OTHER render slots
                              ; at $9c36-$9d0c, so once per VBL)
$9d24  move.b $6c5d,D0        ; D0 = $6c5d (PRNG byte; discr-b6x's own hunt)
$9d28  add.b  $6ab5,D0        ;      + $6ab5
$9d2c  move.b D0,$6c5d        ; advance the PRNG
$9d30  and.b  #$7f,D0         ; D0 &= 0x7f  (0..127)
$9d34  cmp.b  #4,D0  / blt $9ab6     ; D0 <4  (4/128)  -> no bonus
$9d3c  cmp.b  #8,D0  / bge $9d4c
$9d42  move.w #2,$6e3a / bra $9aea   ; 4<=D0<8  (4/128) -> code 2
$9d4c  cmp.b  #$a,D0  / bge $9d5c
$9d52  move.w #1,$6e3a / bra $9aea   ; 8<=D0<$a (2/128) -> code 1
$9d5c  cmp.b  #$c,D0  / bge $9d6c
$9d62  move.w #4,$6e3a / bra $9aea   ; $a<=D0<$c(2/128) -> code 4 (shield)
$9d6c  cmp.b  #$e,D0  / bge $9d7c
$9d72  move.w #5,$6e3a / bra $9aea   ; $c<=D0<$e(2/128) -> code 5
$9d7c  bne.b  $9d88                  ; D0==$e exactly (1/128) -> code 3
$9d7e  move.w #3,$6e3a / bra $9aea
$9d88  rts                           ; D0 15..127 (113/128) -> no bonus
```

11/128 (~8.6%) of rolls mint a code; the other 117/128 do nothing. Every
`bra.w` on a successful roll lands at `$9aea`, which picks WHERE:

```
$9aea  clr.l  $6e32           ; bonus-icon render state, zeroed
$9aee  st.b   $6e16           ; icon-active flag
$9af2  move.b $6c5d,D0 / add.b $6ab5,D0 / move.b D0,$6c5d   ; SAME prng, rolled again
$9afe  and.w  #7,D0           ; D0 = 0..7 -- which of 8 eligible cells
$9b02  move.w D0,$6e36        ; recorded (also reused/overwritten afterwards
                              ; by the icon's own per-frame animation code,
                              ; so $6e36 at rest does not reliably name the
                              ; cell in hindsight -- read the grid instead)
$9b08  lea $5028,A1 / movea.l (0,A1,D0*4),A1   ; per-slot render-state table
$9b10  ...                    ; copies 3 fields from it into $6e1c/$6e24/$6e20
$9b1c  move.w #$fa,$6e38      ; icon on-screen lifetime, 250 frames (~5s)
$9b22  lsr.w #2,D0 / mulu.w #8,D0        ; D0 back to slot, then *8 (cell stride)
$9b28  lea $761e,A0
$9b2c  or.w  #$0080,($02,A0,D0.w)        ; **SETS BIT 7** at $761e+2+8*slot
$9b32  lea $759e,A0
$9b36  or.w  #$0080,($02,A0,D0.w)        ; and again at $759e+2+8*slot (far bank)
$9b3c  rts
```

`$761e` is `$7616+8` and `$759e` is `$7596+8`, so both ORs land on cell
`slot+1` of their own bank (confirmed against this fixture: the observed
placement was cell 7, i.e. slot 6). **Both banks are written unconditionally
in the same straight-line sequence** -- a placement is not near-bank-only.
This fixture's own live capture only shows bit 7 on the near bank at the
moment of seeding; the far bank's copy was already gone by then in an
earlier, slower diagnostic pass (confirmed by re-running with the seed taken
immediately on detection instead, no intervening debugger round trips --
the near-bank flag then reads clean, see "Reproducibility" above). Whether
that is the opponent's own play consuming the far bank's copy in real time,
or bit 7 only being meaningful on the bank a live disc can currently strike,
is not resolved here and is not needed for either accepting bead.

**discr-ovl.4 accepted**: the writer of bit 7 (`$9b2c`/`$9b36`) and the writer
of `$6e3a` (`$9d42`/`$9d52`/`$9d62`/`$9d72`/`$9d7e`, gated by the `$6e3c`
timer at `$9d16`-`$9d1e`) are both named, address for address, and this
fixture is the trace in which a bonus is picked up.

## discr-z8m: the writer of `$6d9a`, named -- semantics still open

`$a2b0  move.w D0w,$6d9a` (inside the near-grid pickup, `$a292`-`$a2ca`,
already decoded in Part 10) is `$6d9a`'s only writer -- it copies whatever
`$9d1c`-`$9d88` most recently minted into `$6e3a`. **Not a multiplier
variable being written directly; the bonus CODE is copied in, exactly as
Part 10's own retraction already established.** This fixture's roll produced
code 2, not code 1, so the `$a314 cmp.w #1,$6d9a` double-apply path
(`$a31c`) -- the actual "multiplier" behaviour the bead was filed against --
is still not exercised by any committed trace. Left open pending a code-1
roll; the mechanism to get one (poll `$6e3a` idle, seed on sight) is proven
and reusable, just not yet lucky. See `reports/part12-bonus.md` for the
hunt budget spent trying.

## The oracle-vs-Hatari divergence (frame 152 on)

Past the validated window, `$6c5d` itself disagrees (Hatari `$53` vs oracle
`$df` at the first diverging frame) -- not an off-by-one, a different
PRNG state entirely. `$6c5d` is exactly the byte this bonus roll advances
on every gate-fire, and this is the FIRST fixture that ever exercises that
code path (every prior trace had `bonus_6d9a` = 0 throughout, meaning the
roll's outcome was never load-bearing before). The likely cause is the
project's own previously-documented category (Part 11f-g): Musashi is
instruction-accurate, not cycle-accurate, and this game's main loop can run
a variable number of passes per VBL depending on cycle budget -- if the
roll's gate (`$6e3c`) is serviced once per pass rather than once per VBL,
a one-pass-per-VBL assumption anywhere upstream would desync `$6c5d` at
exactly the frame the pass count first differs. Not chased further here:
it sits after the event this fixture exists to prove, and is a `discr-b6x`
question (the PRNG hunt), not an `ovl`/`z8m` one.

## disc-core's own gate

`tracecheck --skip-waived` (players[1].\* resynced per `discr-b6x`): **150
ticks** match, then `tiles[7].hp` diverges -- the ST/oracle moves 132 -> 4
(bit 7 stripped on pickup), disc-core moves 132 -> 129 (`132 - 3`, the plain
damage subtraction `tile::damage` already implements, oblivious to bit 7 as
anything but a very large hp value). This is precisely the gap discr-ovl.4
found and discr-z8m still needs code 1 to finish measuring: disc-core has
no model at all yet for a bit-7-carrying cell, the bonus table at `$9aa2`,
or `$6e3a`/`$6d9a`.

Without `--skip-waived`: **22 ticks** -- player 2's AI enters one of the 28
states at `$c6ec` this crate has no handler for (`discr-b6x`), same
pre-existing limitation `golden.ndjson`/`tile_damage.ndjson` waive around.
Not new, not caused by anything in this fixture.

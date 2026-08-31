# Part 14 (psprites) -- PLAYER01/ENEMY01 pixel format cracked by reading the blitter, wired into disc-app

bd `discr-rxx.7`'s last gap, following wave 9's negative result
(`reports/part13-sprites.md`: 24 plane-order permutations, width sweeps,
mask hypotheses, best template-match score ~27%, barely above chance).
That pass's own lesson -- guessing the layout doesn't work here, read the
game's own sprite-drawing code instead -- is exactly what this pass did.

**Delivered**: the PLAYER01/ENEMY01 pixel sub-format, pinned by
disassembling the actual sprite blitter out of `discram.bin` (not guessed),
proven by a decode that is pixel-for-pixel identical to a live Hatari
screenshot's own palette indices, and wired into `disc-app` so both players
render from the real depacked art in place of the flat-rectangle
placeholder. Bead closed.

## 1. Finding the blitter, not guessing the data

`scripts/ghidra/q.sh` (an existing, pre-built query harness over a Ghidra
analysis of `discram.bin` as a raw 68000 image, `scripts/ghidra/{q.sh,Q.java,
env.sh,import.sh}`, seeded from `docs/disc-notes.md`'s already-known code
addresses) makes this a few commands, not a fresh disassembler build:

```
sh scripts/ghidra/q.sh scan 6ce4
```

`$6ce4` is the word the animation tail (`$f1ca`, already documented in
`docs/disc-notes.md` Part 12 -- "The frame block: two fields at fixed
offsets, the rest sprite data") copies a frame block's first long into,
purely as a change-detection sentinel. `scan` lists every instruction
anywhere that touches that address as an operand -- 20 hits are the
animation-tail copies already known from Part 12, and one, at `$12dcc`, is
not a copy: it's `movea.l ($6ce4).w,A0`, using that same value **as an
address**, followed by a full row-blit loop. That's the actual sprite
draw, in a code region no earlier pass's analysis had a function boundary
for (`fun` doesn't list it; `dis 12d90 90` disassembles it directly by
address).

## 2. The blitter, disassembled

Entry (`$12d30`-`$12dcc`, decompiled by hand from `dis`'s raw asm): reads
the player's world X/Y/Z, projects through two byte-indexed LUTs at
`$59952` and `$5b252` (**the CONVERTX/Y.DAT consumption site**
`reports/part13-art.md` went looking for and mismatched -- a live one,
found as a side effect of reading this routine; not chased further here,
filed as a follow-up below), then computes a screen address via a row table
at `$6aba` + base `$6aac` (the same `screen_buf_a` `docs/disc-notes.md`
already names) and a sub-pixel shift pair `D5 = x&0xF`, `D6 = 16-D5`.

Then the part that matters for the pixel format:

```
$12dcc  A0 = ($6ce4).l          ; frame block's own first long: a full
                                 ; 32-bit ABSOLUTE pointer to pixel data,
                                 ; not "category:offset" as guessed earlier
$12dd0  D7 = ($6cd6).w - 1      ; row counter, frame block +$04 (height)
$12dda  tst.b ($6cb5).w         ; frame block +$15
$12dde  bne.w $12f7c
-- $12dec (cb5==0): 4 colour planes + 1 mask word, 16px wide, 10 B/row
-- $12f7c (cb5!=0): 3 colour planes + 1 mask word, 32px wide, 16 B/row
```

Both bodies are the same shape, disassembled and decompiled in full
(`sh scripts/ghidra/q.sh dis 12dcc 90 dis 12f7c 90 dis 13040 200`): per
16px word-group, `planes` data words then **one mask word last** (not
first), MSB-first; `and.w mask,(A1)` then `or.w data,(A1)+` per plane --
opaque wherever the mask bit is 0, background left alone where it's 1.
`ror.l D5`/`lsr.l D6` split each source word across two destination words
for sub-pixel horizontal positioning (three write groups per row, `lea
$88/$80/$90,A1` picking the exact row-stride variant -- these three total
136/128/144 + the 24 bytes already advanced in-loop = 160/152/168; the
`cb5!=0` path's own two samples both land on `cb4==0` -> stride 160, the
plain 320x200x4bpp scanline byte count, confirming this genuinely writes
the live screen bitmap). None of that runtime-positioning machinery
matters for decoding one frame's pixels in isolation, so
`decode_sprite_frame` fixes the shift at 0 and ignores frame block +$14/
+$20 (`cb4`, unidentified beyond "selects a row-stride variant"; every
frame this pass sampled reads 0 there).

**Frame block layout**, now complete (extends Part 12's "two fields at
fixed offsets, the rest sprite data" with what those first 10 bytes are):

| offset | field | source |
|---|---|---|
| `+$00` | `u32` sprite pixel pointer (absolute ST address) | this pass |
| `+$04` | `u16` height (rows) | Part 13 (byte-arithmetic), confirmed here |
| `+$06` | `i16` unidentified | Part 13 named, unchanged |
| `+$08` | `i16` unidentified (~`-(height-1)`) | Part 13 named, unchanged |
| `+$0a` | `i16` `x_delta` | Part 12 |
| `+$0c`-`+$13` | `[i16;4]` `hit_box` | Part 12 |
| `+$14` | `u8` `cb4` -- selects one of 3 row-stride variants (`136`/`128`/`144`) | this pass |
| `+$15` | `u8` `cb5` -- selects 4-plane/16px vs 3-plane/32px pixel format | this pass |

## 3. Decode, proven against real bytes and a real screenshot

Implemented in Python first (`/tmp` scratch, not committed) against
`discram.bin` directly, then in Rust (`crates/disc-app/src/gfx.rs`,
`decode_sprite_frame`). Sampled every catalogued player-1 sequence this
crate has a table for -- idle (`$16ce`, height 48), struck-down (`$202c`,
height 44, `hit_box [-4,11,-19,16]`), dead (`$2084`, height 34), walk-left
(`$1794`, height 45) -- plus player 2's idle (`$30e2`) and struck-down
(`$3a40`). Every one decodes to a crisp, immediately recognisable humanoid
sprite in the *correct* pose (standing back view; arms-out falling; feet-
apart collapsed; mid-stride side profile) with **zero per-frame tuning**.
Byte counts close exactly against Part 13's own independent arithmetic
(idle: 768 B = 16 B/row x 48 rows, matching the previously-proven "idle
frames differ by 768" delta).

Player 2's frame-block pointer for its idle sequence resolves to `$4ac2c`
-- inside ENEMY01's own resident span `[$4A2F4, $59952)`
(`reports/part13-codec.md`'s proof-pair table), confirmed programmatically
(`PLAYER01_RAM_ADDR + PLAYER01_LEN == ENEMY01_RAM_ADDR`, the two assets are
back-to-back). So in this 1-vs-1 game, "player 2" and "the enemy" are the
same on-screen character, and it draws through the `cb5==0` (16px,
4-plane) path while player 1 draws through `cb5!=0` (32px, 3-plane) --
both formats needed live verification, not just one.

**Live screenshot comparison** (the house rule's actual bar): resumed
`tmp/match_training.sav` (copied from the main checkout; `scenarios/
sprite_shot.yaml`, already committed by the prior sprites shift) via
`python3 scripts/collect.py --scenario scenarios/sprite_shot.yaml`, giving
a real in-match screenshot (`tmp/shots-sprite_shot/reference.bmp`, this
worktree, not committed) and a fresh 16-colour palette read straight off
`$FF8240`-`$FF825E` (`dumps/sprite_shot/palette.bin`). Player 1's idle
frame 0, decoded with that same palette and cropped/aligned against the
screenshot's own standing player by search, is **pixel-for-pixel
identical**: a 32x48-window offset search over the screenshot (accounting
for its 2x-doubled overscan capture, `reports/part13-sprites.md` already
established this) found an alignment where **all 453 of the frame's 1536
pixels that the mask marks opaque match the screenshot's own nearest-
palette index exactly -- 100%**, up from the prior pass's ~27% (chance
level, ~19% expected). Not a rough visual read: an exact index match, at
every opaque pixel, once decode + palette + alignment are all correct.
Side-by-side crop: `tmp/shots/side_by_side.png` (this worktree, not
committed) -- the real screenshot's own player next to the freshly decoded
frame, same pose, same shading, same silhouette.

## 4. Wired into `disc-app`

`crates/disc-app/src/gfx.rs` (extends the module the DALLES01 tile decoder
already lives in):

- `decode_sprite_frame(ram, frame_block_addr) -> Option<(width, height,
  Vec<u8>)>` -- the format above, generic over the two plane-counts/widths,
  `SPRITE_TRANSPARENT` (`0xff`, not a valid palette index) marking masked-
  out pixels. Self-checked (`tests`) against a hand-built synthetic frame
  round-tripped into the exact byte layout, same method every decoder in
  this module already uses before being trusted against real data; also
  checked to fail closed (`None`, never panic) on truncated RAM or a bogus
  height.
- `frame_block_addr_for_cursor(ram, cursor) -> Option<u32>` -- resolves
  `disc_core::types::Player::anim_cursor` (already `pub`, already exactly
  `anim_base + 6*anim_cell` per `disc_core::player`'s own "cursor rule",
  Part 12) to the frame block it points at, so no new pointer table needed
  duplicating in `disc-core` -- the frame block table lives in the game's
  own code/data segment, not inside either depacked asset, so it is read
  live out of the same RAM-image dev fallback everything else in this
  module already uses.
- `sprite_asset_for_ptr` -- a defensive gate: a resolved sprite pointer
  outside both `[PLAYER01_RAM_ADDR, +LEN)` and `[ENEMY01_RAM_ADDR, +LEN)`
  is skipped rather than decoded, so a stale or unrelated RAM capture
  produces a missing texture (falls back to placeholder) instead of a
  decode of coincidentally-parseable garbage.
- `depacked_ram_image() -> Option<Vec<u8>>` -- the full flat RAM image,
  unsliced (unlike `depacked_dalles01`'s one-asset slice): the frame-block
  table and both assets' pixel data share one address space, so a fixed
  single-asset slice can't serve this decoder. Same gitignored dev
  fallback as the rest of the module.
- `SpriteAtlas` -- builds one `Texture2D` per `anim_cursor` every sequence
  in `disc_core::player::ANIMS` can take, once, up front (`draw_match`
  takes `&MatchState`, so texture lookup at draw time has to be read-only,
  same reasoning `TileAtlas` already documents). `Default` (empty map)
  keeps every `#[cfg(test)]` `MatchState::new` call site working.

`crates/disc-app/src/main.rs`: `MatchState` gains a `sprites: SpriteAtlas`
field and `with_sprites` builder (mirrors `with_gfx`/`with_sfx`); the
player-drawing loop in `draw_match` looks up `ms.sprites.texture(q.
anim_cursor)` -- `q.anim_cursor` already **is** the frame's own identity,
so no state-to-pose mapping is invented (unlike `tile_shape_for_hp`'s
documented display heuristic) -- and draws the real texture, scaled by one
fixed ST-pixel-to-world-unit constant (not normalised per frame, so a
frame's own height still reads as pose: idle's 48px stands tall, dead's
34px visibly shorter) in place of the flat rectangle. `None` (RAM image
unavailable, or this cursor isn't one any catalogued sequence uses) falls
back to the exact original placeholder rectangle, notch included,
unchanged.

**Runtime proof**: `disc-app` built in release and run under Xvfb +
`xdotool` (menu -> Space -> a live TRAINING match), `DISCR_RAM_IMAGE`
resolved via the module's own default path
(`<worktree>/tmp/ghidra_proj/discram.bin`, copied in from the main
checkout for this run, gitignored, never committed).
`tmp/shots-psprites-app/02_after_space.png` and `03_running.png` (this
worktree, not committed) show both players as real orange humanoid
sprites -- player 1 crouched in a struck/collapsed pose after a round
ended, player 2 standing -- in place of the flat-rectangle placeholder, live
in the running app, across two different match states. No crash; the
known-nonfatal ALSA "Can't open PCM device" background-thread panic (no
audio device in this sandbox, `reports/part13-sprites.md` documents the
same thing) is unrelated and does not affect rendering.

## 5. Follow-ups filed, not chased this pass

- The projection LUTs at `$59952`/`$5b252` this blitter itself reads are
  very likely the actual CONVERTX/Y.DAT consumption site
  `reports/part13-art.md` went looking for and found a mismatch at --
  live-verifying that (the same PC-breakpoint method the depack report
  used) and comparing against `disc-app`'s current linear projection is a
  natural rxx follow-up, not attempted here (out of this bead's scope).
- Frame block `+$06`/`+$08` (two `i16`s between height and `x_delta`) and
  `+$14` (`cb4`, the row-stride-variant selector) remain unidentified
  beyond what they structurally do in the blitter -- none of the frames
  this pass sampled needed anything but the `cb4==0` default, so their
  full meaning (do any catalogued frames use the other two stride
  variants? what do `+$06`/`+$08` mean semantically?) is untouched.
- Facing mirroring: player 1's `ANIM_P1_WALK_LEFT` is the only walk table
  `disc_core::player` carries (walking right reuses the same state
  handlers with `addq` instead of `subq`, per that module's own doc); this
  pass renders whichever pose the cursor names as-is, with no horizontal
  flip for the mirrored direction. Visually correct for every pose that
  isn't direction-sensitive (idle, struck, dead) and for the one direction
  each asymmetric sequence's own art was authored for; a future pass
  wanting the mirrored direction pixel-correct too would need to find
  where facing selects a flip (a candidate: the blitter's own `cb4<0`
  third branch, `$13094`, not chased this pass since no sampled frame used
  it).

## Gates run

```
cargo fmt --check                                                     -- clean
cargo clippy -p disc-app --all-targets -- -D warnings                 -- clean
cargo test -p disc-app                                                -- 51 passed, 0 failed
cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/golden.ndjson --min-agree 99
                                                                        -- OK: 99 tick(s) matched (core untouched)
```

## Files

- `crates/disc-app/src/gfx.rs` -- `decode_sprite_frame`,
  `frame_block_addr_for_cursor`, `sprite_asset_for_ptr`,
  `depacked_ram_image`, `PLAYER01_RAM_ADDR`/`LEN`, `ENEMY01_RAM_ADDR`/`LEN`,
  `SPRITE_TRANSPARENT`, `SpriteAtlas` + tests; module doc extended with the
  format spec and disassembly transcript.
- `crates/disc-app/src/main.rs` -- `MatchState` gains a `sprites:
  SpriteAtlas` field and `with_sprites` builder; the player-drawing loop
  draws the real sprite texture keyed by `anim_cursor`, falling back to the
  original placeholder rectangle unchanged when unavailable; `main` loads
  `SpriteAtlas::load()` once alongside `gfx`/`sfx`.
- `reports/part14-psprites.md` -- this file.

Not committed: `tmp/shots-sprite_shot/`, `tmp/shots-psprites-app/`,
`tmp/psprites_app.log`, `dumps/sprite_shot/` (this worktree's own runtime
captures); `tmp/ghidra_proj/discram.bin`, `tmp/Disc (1990)(Loriciel)[cr
Exo-7].st`, `tmp/emutos-512k-1.4/`, `tmp/match_training.sav` (pre-existing
ground-truth/emulator inputs, gitignored, copied in from the main checkout,
read-only, never embedded or committed, per the bead's own instructions).

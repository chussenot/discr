# Part 13 (sprites) -- DALLES01 wall-icon tiles proven and wired; PLAYER01/ENEMY01 not cracked this pass

bd `discr-rxx.7` ("disc-app: real assets -- original sprites, tiles and
samples"), third shift on this bead after `formats`/`depack`'s wave-8-9 work
(`reports/part13-depack.md`, `reports/part13-depack2.md`) and `art`'s prior
pass (`reports/part13-art.md`, samples wired, DECOR00/BONUS01/VIC.DAT pixel
format not cracked). Scope this shift: the three files wave-9's depack
report proved go through the Ice! codec and are depacked to known RAM
addresses (`DALLES01`, `PLAYER01`, `ENEMY01`) -- working from the RAM copies
directly (`tmp/ghidra_proj/discram.bin`), sidestepping the still-unresolved
bit-exact codec entirely, per the bead's own "key insight."

**Delivered**: DALLES01's pixel format, proven by six instantly-recognisable
wall-icon shapes matching the manual's own damage cycle, wired into
`disc-app` as a real-time-decoded texture atlas replacing the flat-rectangle
tile rendering, screenshotted live in the running app. **Not delivered**,
evidence recorded rather than a guess shipped: PLAYER01/ENEMY01's pixel
sub-format -- the frame-block `offset`/`height` fields ARE confirmed, but no
width/plane-order/offset combination this pass tried reproduced a
recognisable sprite (best template-match score ~27%, barely above chance).

## 1. Ground truth: palette and a reference screenshot

`scripts/collect.py`'s `Hatari` class, resumed from the cached
`tmp/match_training.sav` savestate (no cold ~40s boot needed), gave two
things a fresh session's own script (not committed; see the module's own
pattern) grabbed in one pass:

- A live screenshot of a real training match (`tmp/shots-sprites/
  sprites_ref_training.bmp`, this worktree, not committed) -- a running
  player mid-stride and the arena's wall-icon boxes, both later used as
  ground truth.
- The live 16-colour palette, `savebin`'d directly off hardware registers
  (`$FF8240`-`$FF825E`, 16 words, standard ST 9-bit `0RGB`) -- the same
  method `reports/part13-art.md` §3 used. Decoded (`crates/disc-app/src/
  gfx.rs::st_color_to_rgb`, `r*255/7` per channel) and carried as
  `gfx::TRAINING_PALETTE`, a 32-byte constant (ground truth, not derived
  pixel data -- same footing as `player.rs`'s hand-transcribed frame
  tables).

Hatari's screenshot geometry: `nZoomFactor=1` with `bAllowOverscan=TRUE`
produces an 832x552 BMP that is a 2x-doubled 416x276 raw overscan frame, so
one real ST pixel is a 2x2 block in the screenshot -- used below to build a
palette-index ground-truth grid at native resolution for the player-sprite
search.

`scenarios/sprite_shot.yaml` reproduces this capture (`python3 scripts/
collect.py --scenario scenarios/sprite_shot.yaml`) -- a screenshot plus a
`dump` step scoped, via the scenario's own `range`, to `$FF8240`-`$FF8260`
instead of the usual `$0`-`$8000` game-state window every other scenario in
this directory dumps. Re-running it during landing produced a palette a few
entries different from the one `TRAINING_PALETTE` carries (indices 4-7 and
12-15 specifically) -- the wall-icon boxes' own background palette entries
cycle frame to frame (a shimmer effect), so any single capture is a valid
snapshot, not a fixed constant of the game. `TRAINING_PALETTE` is the
snapshot this pass's renders and screenshots were actually validated
against; a future pass wanting the animation itself would need to capture a
short run of frames, not a single `dump`.

## 2. The pixel format: standard ST low-res, proven on DALLES01

**Format**: 4 contiguous interleaved bitplanes per 16px word-group,
word-column-major (all 4 plane words for pixels 0-15, then all 4 for pixels
16-31, ...), MSB-first bit order (bit 15 of each plane word is that group's
leftmost pixel), plane 0 -> index bit 0 up to plane 3 -> index bit 3. No
mask channel. This is the plain hardware-native ST low-res raster -- and
matches the depack report's own disassembly finding for the Ice! routine's
last stage (`reports/part13-depack2.md`: a 4-way bit-scatter accumulator
ending `movem.w d0-d3,(a3)`, "structurally a chunky/packed-nibble to
ST-native-4-bitplane format converter"), i.e. the RAM copy needs no further
transform once depacked.

**How it was pinned**: not by inference from the depack report alone --
by brute-force width search directly against DALLES01's own depacked bytes
(`$1D41C`, 31664 B), decoding wide swaths at every 16px-multiple width and
looking for real art. Width 32px (2 words) was immediately, unambiguously
right: the very first ~300 rows resolve into a sequence of crisp, bold icons
that are instantly recognisable by eye -- an orb/disc icon, a
flame/mountain-silhouette texture band, then, starting exactly at row 48 and
repeating every 16 rows thereafter (confirmed programmatically: per-row
dominant-colour signature is periodic with period 16 across rows 48-143,
`scripts` used this pass, not committed -- see §5), **six** icons in a row:

| index | row range | shape |
|---|---|---|
| 0 | 48-63 | equals (two horizontal bars) |
| 1 | 64-79 | triangle |
| 2 | 80-95 | square (nested square) |
| 3 | 96-111 | pentagon |
| 4 | 112-127 | circle |
| 5 | 128-143 | lemniscate (infinity symbol) |

This is the manual's own wall-tile damage cycle
("pentagon->square->triangle->equals->circle->lemniscate", just recovered in
a different sequential order in the data) reproduced byte-for-byte out of
the depacked RAM image with a single, uniform decode -- no per-icon tuning,
no palette reshuffling between icons. See `tmp/shots-sprites/
dalles_six_strip.png` (this worktree, not committed) for the six icons
rendered side by side; `tmp/shots-sprites/sprites_ref_training.png`'s own
wall-icon boxes (one lit orange disc, seven infinity/"∞" symbols -- Part 13
first shift's own observation, `reports/part13-art.md` §3) are exactly this
same DALLES01 art, live in the real game.

Past row 144 the same decode keeps paying off: a solid-orange floor tile
with dark-red vein/marble texture (matching the real screenshot's floor
squares), then a gem/oval icon and a diamond icon (bonus-tile candidates),
all clean, all recognisable, all from the identical decode with no
special-casing.

**Rejected as red herrings before landing on width 32**: plain 4-plane
chunk-within-row at width 48 and 64 (recognisable-*looking* bands turned out
to be a Moire/shearing artifact of a wrong width when cross-checked -- width
64 showed the same motif duplicated twice across its span, the tell of a
too-wide read); a masked (mask + 3 colour-plane) 16px-wide interpretation
(structurally clean per-row dumps by hand, but never assembled into a
coherent image at any height); several planes<->bit permutations (24 of
them, contact-sheeted, all identical silhouette, only recoloured -- ruling
out plane order as the fix for anything). The width=32, 4-plane, no-mask,
standard order was the *first* thing tried on DALLES01 and it was
unambiguously right immediately -- the extended search was spent on
PLAYER01, where it never worked (§4).

## 3. Wired into `disc-app`

`crates/disc-app/src/gfx.rs` (new):

- `decode_planar(bytes, width_words, height, planes) -> Option<Vec<u8>>` --
  the decoder above, generic over geometry, self-checked in `tests` against
  a hand-built synthetic image round-tripped by hand into the exact byte
  layout it consumes (the same method `reports/part13-art.md` used before
  trusting a decode against real data).
- `TileShape` (6 variants) + `tile_shape_indices` -- the row-math table
  above, `#[test]`-covered for the short-input and correct-length cases.
- `depacked_dalles01() -> Option<Vec<u8>>` -- the one function-call swap
  point for codec's real decoder. Today: reads the depacked span directly
  out of a local `discram.bin` RAM image if the worktree has one (default
  path `<crate>/../../tmp/ghidra_proj/discram.bin`, overridable with
  `DISCR_RAM_IMAGE`) -- gitignored runtime state, never `include_bytes!`'d,
  never committed. Once discr-rxx.5 lands `depack_ice()`, this function's
  body becomes `disc_tools::depack_ice(&std::fs::read(DALLES01.DAT)?)` and
  every caller (one: `TileAtlas::load`) is unaffected -- see the module's
  own doc comment for the exact diff shape.
- `TileAtlas` -- builds 6 `macroquad::texture::Texture2D`s (nearest-filtered)
  from the decode + `TRAINING_PALETTE`, once, in `main()`; `Default` (no
  textures) keeps every existing `#[cfg(test)]` `MatchState::new` call site
  working unchanged, same shape as `audio::Sfx`.

`crates/disc-app/src/main.rs`: `MatchState` gains a `gfx: TileAtlas` field
and `with_gfx` builder (mirrors `with_sfx`); `draw_bank` takes `&TileAtlas`
and, for a live (non-destroyed, non-collapsing) tile, draws the atlas
texture stretched to the cell instead of the flat wear-shaded rectangle --
falling back to the exact original rectangle when the atlas didn't load
(`TileAtlas::texture` returns `None`). Which of the six shapes to show is
`tile_shape_for_hp(hp) = TileShape::ALL[hp.clamp(0,5)]` -- **explicitly
documented as a display heuristic, not a decoded fact**: this pass did not
live-verify which ST code site reads DALLES01's shape index at draw time
(follow-up, same as `CONVERTX/Y.DAT`'s still-open consumption site,
`reports/part13-art.md` §4/§5), so the exact `hp`->shape formula is unknown;
cycling through the manual's own six shapes as `hp` drops is a reasonable
stand-in, clearly labelled as such in both the doc comment and this report.
Destroyed-tile outlines, the collapse-flicker animation and the bonus-flag
gold dot are all unchanged.

**Runtime proof**: `disc-app` built and run under Xvfb + `xdotool` (menu ->
Space -> a live TRAINING match), `DISCR_RAM_IMAGE` pointed at the shift's own
`discram.bin` capture. `tmp/shots-sprites-app/02_after_space.png` (this
worktree, not committed) shows the purple marbled DALLES01 tile texture
rendering on all 8 cells of both banks, live, in place of the flat
rectangles -- `tmp/shots-sprites-app/tile_zoom.png` crops one cell 4x and
the icon's paired horizontal bars (the "equals" shape, `hp.clamp(0,5)`
having selected index 0 for a fresh match's starting `hp`) are visible. No
crash, no missing-asset warning (the RAM image was present this run); the
`None`-atlas fallback path is what every `#[cfg(test)]` `MatchState`
exercises instead, and all 45 `disc-app` tests still pass.

## 4. PLAYER01/ENEMY01: extensive search, no format found

Player-sprite animation frames are addressed through `crates/disc-core/src/
player.rs`'s already-decoded frame-block tables (Part 12): each 20-byte
frame block is `[10 B sprite data][x_delta: i16][hit_box: [i16;4]]`, and this
pass's own read of that first 10 bytes (`word0, word1, word2, word3, word4`)
confirms, independent of any pixel-format hypothesis:

- **`word1` is a byte offset from PLAYER01's own depacked base (`$24FCC`)**
  to that frame's own sprite bytes. Confirmed by the "P matches file size"
  logic depack2 already established for the container as a whole, and
  independently by **byte-size arithmetic**: the difference between two
  frames' `word1` values, across seven independent frame pairs spanning four
  different animation categories (idle/walk/struck-down/dead), equals
  exactly `16 * (the earlier frame's own word2)` every time (e.g. idle
  frames differ by 768 = 16x48; walk by 720 = 16x45; dead by 544 = 16x34).
  Zero discrepancies across all seven pairs -- this is not a guess.
- **`word2` is that frame's sprite height in rows** -- the same arithmetic
  above only closes if it is; it also tracks sensibly against the DEAD
  sequence's own shrinking `hit_box` height as the character collapses.
- **16 bytes is therefore each row's fixed byte count** for every frame
  tried, regardless of animation category.
- `word0` (either `2` or `3` depending on animation category, never
  anything else) and `word3`/`word4` (small signed values, `word4`
  tracking roughly `-(height-1)`, consistent with a feet-anchored draw
  origin) remain unidentified -- not width, not proven to be anything else
  either.

**What did not work**: 16 bytes/row, at every geometry consistent with that
byte count (16px-wide mask+3-plane, 32px-wide plain-4-plane, 32px-wide
mask+3-plane, 48px and 64px plain-4-plane), decoded at the frame-table's own
offset, produced either an unrecognisable shape or (once a systematic
"wishbone" silhouette recurred **identically** across the idle table, the
dead table, and unrelated offsets ~10 KB apart -- itself strong evidence of
an aliasing artifact rather than real content) a shape that could not be
anatomically explained by any pose the manual or a real screenshot showed.
Because permuting the four planes' bit-to-colour-channel assignment (all 24
permutations, both bit orders, 48 renders total, contact-sheeted) reproduces
that exact same silhouette every time -- only recoloured, never
reshaped -- the plane order is not the variable at fault.

**The quantitative check that closed this pass**: a small ground-truth
palette-index grid (35x30 real ST pixels) was extracted from the reference
screenshot's own running player (native-resolution downsample, nearest-match
against `TRAINING_PALETTE`) and used as a template. A search over width in
{16,32,48,64}px, byte offset spanning the full range from idle through
struck-down's own table entries (`0x7800`-`0x8800`), and plane count in
{3,4} with standard order, scored each candidate by the fraction of the
ground truth's own body-coloured pixels (indices `{2,3,4}` -- the player's
own observed palette, black outline excluded) the candidate reproduces at
the same position. **Best score across the entire search: ~27%**, against
an estimated ~19% for chance agreement with a 3-of-16-colour target -- not a
match. (An earlier, cruder version of this search, scoring plain
"pixel-in-allowed-set" without position alignment, was found to be gamed by
long runs of zero bytes elsewhere in PLAYER01 and discarded once diagnosed.)

Two structural possibilities this pass did not chase, both plausible given
the depack report's own leftover open items:
(1) the actual mask/sprite data for player limbs may be one of the depack
report's own "17 more... sprite masks" loads (Finding B), landing at a RAM
address this pass never captured -- i.e. PLAYER01's own `$24FCC` block may
be colour-plane-only, with masks genuinely elsewhere, not absent; (2) the
frame block's first 10 bytes may encode a nested descriptor (an explicit
width/height header) rather than raw pixels starting immediately -- the raw
bytes at every offset tried show no obvious embedded header, but this was
not exhaustively ruled out. Per house rules ("never guess the algorithm
into the code"), neither is landed; both are named here as the concrete next
moves, same footing as `reports/part13-art.md`'s own §5 follow-up for
DECOR00/BONUS01/VIC.DAT.

`main.rs`'s player/enemy rendering is **unchanged** -- still the placeholder
rectangles it always was.

## 5. Codec integration status

`pact msg inbox` checked at task start and again before landing: empty both
times. `codec`'s `depack_ice()` (discr-rxx.5) has not landed this shift.
`gfx::depacked_dalles01`'s dev-fallback (§3) is the only source this pass
had; it is structured so codec's function replaces it as a one-line body
swap with no caller-visible change (see the function's own doc comment for
the exact replacement).

## Gates run

```
cargo fmt --check                                                     -- clean
cargo clippy -p disc-app --all-targets -- -D warnings                 -- clean
cargo test -p disc-app                                                -- 45 passed, 0 failed
cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/golden.ndjson --min-agree 99
                                                                        -- OK: 99 tick(s) matched (core untouched)
```

## Files

- `crates/disc-app/src/gfx.rs` -- new. `decode_planar`, `TileShape`,
  `tile_shape_indices`, `depacked_dalles01`, `TileAtlas`, `TRAINING_PALETTE`,
  tests.
- `crates/disc-app/src/main.rs` -- `MatchState` gains a `gfx: TileAtlas`
  field and `with_gfx` builder; `draw_bank` takes `&TileAtlas` and draws the
  real tile texture (falling back to the original flat rectangle when
  unavailable); `tile_shape_for_hp` (display heuristic, documented as such);
  `main` loads `TileAtlas::load()` once and attaches it to every new
  `MatchState` alongside `sfx`. Player/enemy rendering unchanged.
- `reports/part13-sprites.md` -- this file.

Not committed: `assets/` (read-only, owned by `orchestrator`, unchanged);
`tmp/shots-sprites*/`, `tmp/palette_training.bin` (this worktree's own
`tmp/`, gitignored, referenced above by path); `tmp/ghidra_proj/discram.bin`
(pre-existing ground-truth RAM image, gitignored runtime state, read-only
input -- never embedded or committed, per the bead's own instructions).

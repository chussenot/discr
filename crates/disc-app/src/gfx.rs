//! Original ST graphics: a pixel-format decoder plus the DALLES01 wall-icon
//! tile atlas, proven against the real game (`reports/part13-sprites.md`).
//!
//! # The pixel format
//!
//! Standard Atari ST low-resolution bitmap: `planes` contiguous interleaved
//! bitplanes per 16px word-group (word-column-major, MSB-first bit order --
//! bit 15 of each plane word is that group's leftmost pixel), plane 0
//! contributing index bit 0 up to plane N-1 contributing index bit N-1. No
//! mask channel, no chunky/nibble packing -- the depack routine's own
//! "chunky-to-planar" bit-scatter accumulator (`reports/part13-depack2.md`
//! Finding, `$344`-`$36a`) already produces this exact hardware-native
//! layout as its output, so the RAM copy needs no further transform.
//!
//! [`decode_planar`] is generic over width/height/plane-count and is
//! validated against a hand-built synthetic image in `tests` (the same
//! self-check method `reports/part13-art.md` used) before being trusted
//! against real data.
//!
//! # DALLES01: proven; PLAYER01/ENEMY01: not proven this pass
//!
//! [`decode_planar`] at `width_words=2` (32px), 4 planes, applied to
//! DALLES01's depacked bytes starting at its own resident RAM address
//! (`reports/part13-depack2.md` Finding B: dest `$1D41C`, decompressed
//! length 31664), reproduces the manual's own six-shape wall-icon damage
//! cycle byte-for-byte: equals, triangle, square, pentagon, circle,
//! lemniscate, each a crisp 32x16 icon starting at row 48 of the decode and
//! repeating every 16 rows -- see [`TileShape`] and `reports/part13-
//! sprites.md` for the row math and rendered proof images.
//!
//! The same decoder, swept across every 16-multiple width and a wide offset
//! range anchored on `crates/disc-core/src/player.rs`'s own frame-block
//! pointers into PLAYER01, did **not** reproduce a recognisable player or
//! enemy sprite -- the best template-match score against a real screenshot's
//! own palette-index ground truth was ~27%, barely above the ~19% a random
//! 3-of-16-colour match would score by chance. Per house rules ("a
//! hypothesis is proven by a rendered image that looks right", "never guess
//! the algorithm into the code"), PLAYER01/ENEMY01 stay on `main.rs`'s
//! existing placeholder rendering; the frame-block's `offset`/`height`
//! fields (word 1 / word 2 of the 10-byte sprite-data header
//! `crates/disc-core/src/player.rs::Frame` doesn't carry) ARE confirmed --
//! reproduced exactly by seven independent frame-pair byte-size deltas
//! across idle/walk/struck/dead -- only the raw pixel sub-format inside each
//! frame's own byte span is still open. See `reports/part13-sprites.md` §2
//! for the full negative-result writeup (offsets tried, scores, images).
//!
//! # Depack source: codec's real decoder, or a documented dev fallback
//!
//! [`depacked_dalles01`] is the one function-call swap point: today it reads
//! the already-depacked bytes straight out of a local `discram.bin` RAM
//! image (the same ground-truth capture `reports/part13-depack2.md` and
//! `reports/part13-art.md` used), if the worktree happens to have one --
//! gitignored runtime state, never embedded (`include_bytes!`) or committed.
//! Once discr-rxx.5 lands `depack_ice()`, this function's body becomes
//! `disc_tools::depack_ice(&std::fs::read(assets/original/DALLES01.DAT)?)`
//! and every caller is unaffected. Absent both sources, it returns `None`
//! and callers fall back to today's placeholder rendering -- never a panic,
//! never a build-time dependency on either source being present.

use std::path::PathBuf;

/// Decode one Atari ST 9-bit `0RGB` palette register (3 bits/channel) to
/// 8-bit-per-channel RGB. `word`'s top 4 bits (rarely used, sometimes a
/// shadow/priority flag on later STE hardware) are ignored.
pub fn st_color_to_rgb(word: u16) -> [u8; 3] {
    let r = (word >> 8) & 7;
    let g = (word >> 4) & 7;
    let b = word & 7;
    [
        (u32::from(r) * 255 / 7) as u8,
        (u32::from(g) * 255 / 7) as u8,
        (u32::from(b) * 255 / 7) as u8,
    ]
}

/// The live 16-colour palette this crate renders original graphics with,
/// captured off real hardware registers (`$FF8240`..`$FF825E`, 16 words) via
/// `scripts/collect.py`'s `Hatari.dbg_capture("savebin ... $ff8240 $20")`
/// during a resumed `tmp/match_training.sav` session (`reports/part13-
/// sprites.md`). A palette is 32 bytes of ground truth, not derived pixel
/// data, so -- unlike a rendered atlas -- it is fine to carry as a constant
/// the same way `player.rs`'s frame tables already are.
pub const TRAINING_PALETTE: [u16; 16] = [
    0x0000, 0x0777, 0x0411, 0x0311, 0x0752, 0x0752, 0x0752, 0x0752, 0x0547, 0x0426, 0x0314, 0x0213,
    0x0752, 0x0752, 0x0752, 0x0752,
];

/// Decode a standard Atari ST low-res bitmap out of `bytes`. `width_words`
/// 16px word-groups wide, `height` rows tall, `planes` contiguous
/// interleaved bitplane words per group (`1..=4` for this game's 16-colour
/// mode). Returns one palette index (`0..(1<<planes)`) per pixel,
/// row-major, or `None` if `bytes` is shorter than `width_words * planes *
/// 2 * height`.
pub fn decode_planar(
    bytes: &[u8],
    width_words: usize,
    height: usize,
    planes: usize,
) -> Option<Vec<u8>> {
    assert!(
        planes <= 8,
        "decode_planar: at most 8 bitplanes are representable in a u8 index"
    );
    let stride = width_words * planes * 2;
    if width_words == 0 || height == 0 || bytes.len() < stride * height {
        return None;
    }
    let width = width_words * 16;
    let mut out = vec![0u8; width * height];
    let mut pos = 0usize;
    for y in 0..height {
        for c in 0..width_words {
            let mut words = [0u16; 8];
            for w in words.iter_mut().take(planes) {
                *w = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]);
                pos += 2;
            }
            for bit in 0..16 {
                let bmask = 1u16 << (15 - bit);
                let mut v = 0u8;
                for (p, &word) in words.iter().enumerate().take(planes) {
                    if word & bmask != 0 {
                        v |= 1 << p;
                    }
                }
                out[y * width + c * 16 + bit] = v;
            }
        }
    }
    Some(out)
}

/// DALLES01's resident span inside the depacked RAM image: dest address and
/// decompressed length, both from the Ice!-loader header capture
/// (`reports/part13-depack2.md` Finding B's table).
pub const DALLES01_RAM_ADDR: usize = 0x1D41C;
pub const DALLES01_LEN: usize = 31664;

const TILE_WIDTH_WORDS: usize = 2; // 32px
const TILE_HEIGHT: usize = 16;
const TILE_BYTES_PER_ROW: usize = TILE_WIDTH_WORDS * 4 * 2; // 16
const TILE_SHAPE_ROW0: usize = 48; // first shape-cycle tile's row index

/// `TILE_WIDTH_WORDS * 16`, exposed for callers building a texture/atlas.
pub const TILE_WIDTH: usize = TILE_WIDTH_WORDS * 16;
pub const TILE_HEIGHT_PX: usize = TILE_HEIGHT;

/// The wall-icon damage-cycle shapes DALLES01 carries, in the order they
/// appear in the depacked data (row `48 + 16*index`, `reports/part13-
/// sprites.md`). The manual describes a wear cycle across these six shapes;
/// this crate does not claim to have decoded the ST's own `hp`-to-shape
/// selection formula (see `main.rs`'s `tile_shape_for_hp` doc for the
/// display heuristic used instead) -- only that these six images exist,
/// decode cleanly, and match the manual's own shape names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileShape {
    Equals,
    Triangle,
    Square,
    Pentagon,
    Circle,
    Lemniscate,
}

impl TileShape {
    pub const ALL: [TileShape; 6] = [
        TileShape::Equals,
        TileShape::Triangle,
        TileShape::Square,
        TileShape::Pentagon,
        TileShape::Circle,
        TileShape::Lemniscate,
    ];

    fn index(self) -> usize {
        match self {
            TileShape::Equals => 0,
            TileShape::Triangle => 1,
            TileShape::Square => 2,
            TileShape::Pentagon => 3,
            TileShape::Circle => 4,
            TileShape::Lemniscate => 5,
        }
    }
}

/// Decode one shape's `TILE_WIDTH x TILE_HEIGHT_PX` palette-index grid out
/// of already-depacked DALLES01 bytes (from [`depacked_dalles01`]).
pub fn tile_shape_indices(dalles01: &[u8], shape: TileShape) -> Option<Vec<u8>> {
    let row0 = TILE_SHAPE_ROW0 + shape.index() * TILE_HEIGHT;
    let byte_off = row0 * TILE_BYTES_PER_ROW;
    let slice = dalles01.get(byte_off..)?;
    decode_planar(slice, TILE_WIDTH_WORDS, TILE_HEIGHT, 4)
}

/// Depacked DALLES01 bytes, from whichever source is available.
///
/// See the module doc's "Depack source" section: this is the one call site
/// that becomes codec's real `depack_ice()` once discr-rxx.5 lands. Returns
/// `None` gracefully (never panics) when neither source is present.
pub fn depacked_dalles01() -> Option<Vec<u8>> {
    // TODO(discr-rxx.5): once codec's depack_ice() lands, prefer it here:
    //   let packed = std::fs::read(asset_dir().join("DALLES01.DAT")).ok()?;
    //   return disc_tools::depack_ice(&packed).ok();
    // and drop the RAM-image fallback below to a `cfg(test)`-only path.
    dev_ram_image_slice_at(ram_image_path(), DALLES01_RAM_ADDR, DALLES01_LEN)
}

/// Worktree-relative default location for the ground-truth RAM image,
/// overridable with `DISCR_RAM_IMAGE` (e.g. for a differently-laid-out
/// checkout). Gitignored runtime state -- see `reports/part13-depack2.md`
/// and `reports/part13-art.md` for how it is produced.
fn ram_image_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("DISCR_RAM_IMAGE") {
        let p = PathBuf::from(p);
        return p.exists().then_some(p);
    }
    let p = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tmp/ghidra_proj/discram.bin"
    ));
    p.exists().then_some(p)
}

/// Pure slicing logic, split out from [`ram_image_path`]'s env/filesystem
/// resolution so it is directly testable with an explicit (possibly
/// nonexistent) path -- this crate is `#![forbid(unsafe_code)]`, so tests
/// cannot mutate `DISCR_RAM_IMAGE` via the unsafe-as-of-1.82
/// `std::env::set_var` to exercise the "no image" path.
fn dev_ram_image_slice_at(path: Option<PathBuf>, addr: usize, len: usize) -> Option<Vec<u8>> {
    let data = std::fs::read(path?).ok()?;
    if data.len() < addr + len {
        return None;
    }
    Some(data[addr..addr + len].to_vec())
}

/// The six [`TileShape`] icons, built once into GPU textures. Cheap to clone
/// into a `MatchState` -- `macroquad::texture::Texture2D` is `Arc`-backed
/// internally, same reasoning `audio::Sfx`'s doc gives for `Sound`.
///
/// [`TileAtlas::default`] (no textures) keeps every `MatchState::new` call
/// site -- all of them in `#[cfg(test)]`, which never touches a macroquad
/// graphics context -- working unchanged; only [`TileAtlas::load`] (called
/// once from `main`, after a context exists) can produce `Some`.
#[derive(Clone, Default)]
pub struct TileAtlas {
    textures: Option<[macroquad::texture::Texture2D; 6]>,
}

impl TileAtlas {
    /// Build the atlas from whatever [`depacked_dalles01`] can find. `None`
    /// (falls back to `main.rs`'s existing placeholder rendering) if no
    /// depack source is available or the decode comes up short.
    pub fn load() -> Self {
        let Some(dalles01) = depacked_dalles01() else {
            return Self::default();
        };
        let mut textures: Vec<macroquad::texture::Texture2D> = Vec::with_capacity(6);
        for shape in TileShape::ALL {
            let Some(indices) = tile_shape_indices(&dalles01, shape) else {
                return Self::default();
            };
            let mut rgba = Vec::with_capacity(indices.len() * 4);
            for idx in indices {
                let [r, g, b] = st_color_to_rgb(TRAINING_PALETTE[idx as usize]);
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
            let image = macroquad::texture::Image {
                width: TILE_WIDTH as u16,
                height: TILE_HEIGHT_PX as u16,
                bytes: rgba,
            };
            let texture = macroquad::texture::Texture2D::from_image(&image);
            texture.set_filter(macroquad::texture::FilterMode::Nearest);
            textures.push(texture);
        }
        let textures: [macroquad::texture::Texture2D; 6] = textures
            .try_into()
            .unwrap_or_else(|_| unreachable!("TileShape::ALL has exactly 6 entries"));
        Self {
            textures: Some(textures),
        }
    }

    /// The texture for `shape`, if the atlas loaded.
    pub fn texture(&self, shape: TileShape) -> Option<&macroquad::texture::Texture2D> {
        self.textures.as_ref().map(|t| &t[shape.index()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The self-check `reports/part13-art.md` used before trusting a decode
    /// against real data: a hand-built image, round-tripped by hand into the
    /// exact byte layout `decode_planar` consumes, must come back pixel for
    /// pixel identical to what was encoded.
    #[test]
    fn decode_planar_round_trips_a_synthetic_image() {
        // 32x3 image (2 word-groups, 4 planes), index = (x + y) % 16, encoded
        // by hand into 4-bitplane-interleaved bytes.
        let width_words = 2;
        let height = 3;
        let planes = 4;
        let width = width_words * 16;
        let mut expected = vec![0u8; width * height];
        for y in 0..height {
            for x in 0..width {
                expected[y * width + x] = ((x + y) % 16) as u8;
            }
        }
        let mut bytes = Vec::new();
        for y in 0..height {
            for c in 0..width_words {
                let mut plane_words = [0u16; 4];
                for bit in 0..16 {
                    let x = c * 16 + bit;
                    let v = expected[y * width + x];
                    for (p, word) in plane_words.iter_mut().enumerate().take(planes) {
                        if v & (1 << p) != 0 {
                            *word |= 1 << (15 - bit);
                        }
                    }
                }
                for w in plane_words {
                    bytes.extend_from_slice(&w.to_be_bytes());
                }
            }
        }
        let decoded = decode_planar(&bytes, width_words, height, planes).unwrap();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn decode_planar_rejects_short_input() {
        assert_eq!(decode_planar(&[0, 0, 0, 0], 2, 3, 4), None);
        assert_eq!(decode_planar(&[], 1, 1, 1), None);
    }

    #[test]
    fn decode_planar_handles_single_plane() {
        // 16x1, 1 plane: MSB set means index 1, else 0.
        let bytes = 0b1010_0000_0000_0000u16.to_be_bytes();
        let decoded = decode_planar(&bytes, 1, 1, 1).unwrap();
        assert_eq!(
            decoded,
            vec![1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn st_color_to_rgb_matches_known_values() {
        assert_eq!(st_color_to_rgb(0x0000), [0, 0, 0]);
        assert_eq!(st_color_to_rgb(0x0777), [255, 255, 255]);
        // 4 -> 4*255/7 = 145 (integer division)
        assert_eq!(st_color_to_rgb(0x0411), [145, 36, 36]);
    }

    #[test]
    fn tile_shape_indices_needs_the_shape_cycle_span() {
        // A buffer too short to reach TileShape::Lemniscate's row must fail
        // closed (None), not panic or silently truncate.
        let short = vec![0u8; TILE_SHAPE_ROW0 * TILE_BYTES_PER_ROW];
        assert_eq!(tile_shape_indices(&short, TileShape::Equals), None);

        let enough = vec![0u8; (TILE_SHAPE_ROW0 + 6 * TILE_HEIGHT) * TILE_BYTES_PER_ROW];
        let decoded = tile_shape_indices(&enough, TileShape::Lemniscate).unwrap();
        assert_eq!(decoded.len(), TILE_WIDTH * TILE_HEIGHT_PX);
    }

    #[test]
    fn tile_shape_all_covers_six_distinct_indices() {
        let idxs: std::collections::HashSet<usize> =
            TileShape::ALL.iter().map(|s| s.index()).collect();
        assert_eq!(idxs.len(), 6);
    }

    #[test]
    fn dev_ram_image_slice_is_none_without_a_path() {
        assert_eq!(dev_ram_image_slice_at(None, 0, 10), None);
    }

    #[test]
    fn dev_ram_image_slice_is_none_for_a_missing_file() {
        assert_eq!(
            dev_ram_image_slice_at(
                Some(PathBuf::from("/nonexistent/path/does-not-exist.bin")),
                0,
                10
            ),
            None
        );
    }
}

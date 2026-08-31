//! Original `.SPL` sound effects, decoded at runtime from `assets/original/`.
//!
//! `assets/disch/DSC`'s 11 `*.SPL` entries are headerless signed 8-bit mono
//! PCM (`docs/loriciel-formats.md` §4, `crates/disc-tools/src/bin/dscfs.rs`'s
//! own `samples` subcommand, `reports/part13-dscfs.md`). This crate does not
//! shell out to that tool or commit its WAV output: it reads the checked-in
//! `.SPL` bytes directly at startup and wraps them in an in-memory WAV
//! container (same scale-up dscfs's own `pcm8_to_wav16` uses -- `sample *
//! 256`, exact, no clipping) so macroquad's `audio` feature (`quad-snd`, via
//! `audrey`) can decode them. Nothing derived is ever written to disk.
//!
//! # The event mapping
//!
//! Nine of the eleven samples map to something `disc-core`/this crate's own
//! `round.rs` can actually observe. Two (`DIC13`, `VITRE15K`) are
//! undocumented (`dscfs`'s own `EVENT_MAP` calls them "unknown") and are
//! loaded but never cued -- see [`Cue`]'s doc for exactly which state each
//! wired sample fires on and why.
//!
//! # Sample rate
//!
//! [`RATE_HZ`] is a playback default, not a recovered fact -- the game's true
//! replay rate comes from its own Timer A setup (`docs/disc-notes.md`: "MFP
//! 13, Timer A, ~4.9 kHz PSG sample streamer" for the live-match streamer,
//! not necessarily the offline `.SPL` assets' own authored rate). Same
//! caveat `dscfs samples --help` prints, reproduced here rather than
//! silently assumed.
//!
//! # Missing assets
//!
//! `assets/original/` is checked into this repo, so every `.SPL` load is
//! expected to succeed. If a file is ever missing (a shallow checkout, a
//! deliberately trimmed clone) [`load`] logs a warning per file and leaves
//! that one cue silent -- [`Sfx::play`] is a no-op for an unloaded cue, never
//! a panic, so a clone without assets still runs (see this crate's own
//! `Cargo.toml` comment on why `disc-app` must never fail to build/run just
//! because an optional asset is absent).

use std::path::PathBuf;

use macroquad::audio::{Sound, load_sound_from_bytes, play_sound_once};

/// Playback rate for every decoded `.SPL`. See the module doc's caveat.
const RATE_HZ: u32 = 8000;

/// Where `assets/original/` lives relative to this crate, mirroring
/// `crates/disc-tools/src/bin/dscfs.rs`'s own
/// `concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/original")`.
fn asset_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/original"
    ))
}

/// One event this crate can trigger a sample for.
///
/// Four map onto real `disc-core::Event`s returned by
/// [`disc_core::GameState::tick`] (`Serve`/`Block`/`TileDestroyed`/`Impact`);
/// the rest are edge-detected in `main.rs`/`round.rs` against state this
/// crate already tracks for its own bookkeeping -- `disc-core` itself does
/// not model death, round-over or win/loss (`round.rs`'s own module doc), so
/// there is no core event to hang them off. None of this is a decode of
/// which ST code site queues which sample; it is this crate's own policy for
/// when each ALREADY-DECODED sample is a reasonable fit, exactly the same
/// status as `round.rs`'s other app-level policy choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cue {
    /// `LAUNCH.SPL`. `disc_core::Event::DiscServed`, or
    /// `MatchState::serve_workaround` putting a disc in play (the workaround
    /// calls `disc-core`'s own `disc::serve` directly, bypassing the event
    /// list -- see that method's doc).
    Serve,
    /// `PARADE.SPL`. `disc_core::Event::DiscReflected` -- a disc turning
    /// around (`disc::reflect`, `$a606`). "Block" is dscfs's own
    /// `EVENT_MAP` gloss for `PARADE`; a reflected disc is the one thing
    /// this crate's event stream reports that plausibly fits it.
    Block,
    /// `DESDALLE.SPL`. `disc_core::Event::TileDestroyed` -- direct, exact
    /// match to dscfs's own `EVENT_MAP` gloss.
    TileDestroyed,
    /// `IMPACT.SPL`. `disc_core::Event::TileDamaged` -- a disc struck a
    /// cell and reduced its HP without destroying it: the literal "disc
    /// impact" dscfs's own `EVENT_MAP` gloss names.
    Impact,
    /// `MORT.SPL`. A player's `state_index` entering
    /// `disc_core::player::STATE_DEAD`, edge-detected against the previous
    /// tick's state (same edge `round::Match::observe`'s own `was_dead`
    /// tracks, read independently here so the cue fires the tick death
    /// actually happens rather than the tick `observe` tallies it).
    Death,
    /// `VICTOIRE.SPL`. `round::Phase` transitioning into `GameOver`.
    Win,
    /// `GONG.SPL`. A fresh round being dealt (`round::Phase::RoundOver` ->
    /// `Playing`), including the first round of a new match.
    Round,
    /// `TOUCHDEF.SPL`. A player's `state_index` entering
    /// `disc_core::player::STATE_INTERCEPT` or `STATE_CATCH19` -- the two
    /// states `player.rs`'s own animation tables name as a defensive catch
    /// (`ANIM_INTERCEPT`/`ANIM_P1_CATCH19_COMMIT` docs), the closest fit on
    /// hand to dscfs's "hit_defended" gloss.
    DefendedHit,
    /// `CHUTE.SPL`. A player's `state_index` entering
    /// `disc_core::player::STATE_STRUCK_DOWN` or `STATE_STRUCK_UP` -- a
    /// knock-down, the closest fit on hand to dscfs's "fall" gloss.
    Fall,
}

/// One `.SPL` sample, decoded once at startup. `None` when the source file
/// was unavailable or failed to decode -- see the module doc.
#[derive(Clone, Default)]
pub struct Sfx {
    launch: Option<Sound>,
    parade: Option<Sound>,
    desdalle: Option<Sound>,
    impact: Option<Sound>,
    mort: Option<Sound>,
    victoire: Option<Sound>,
    gong: Option<Sound>,
    touchdef: Option<Sound>,
    chute: Option<Sound>,
}

impl Sfx {
    /// Play `cue`'s sample once, or do nothing if it never loaded.
    pub fn play(&self, cue: Cue) {
        let sound = match cue {
            Cue::Serve => &self.launch,
            Cue::Block => &self.parade,
            Cue::TileDestroyed => &self.desdalle,
            Cue::Impact => &self.impact,
            Cue::Death => &self.mort,
            Cue::Win => &self.victoire,
            Cue::Round => &self.gong,
            Cue::DefendedHit => &self.touchdef,
            Cue::Fall => &self.chute,
        };
        if let Some(sound) = sound {
            play_sound_once(sound);
        }
    }
}

/// Load and decode all nine wired samples (plus the two undocumented ones
/// are simply skipped -- nothing cues them). Call once, before the first
/// match starts.
pub async fn load() -> Sfx {
    Sfx {
        launch: load_one("LAUNCH.SPL").await,
        parade: load_one("PARADE.SPL").await,
        desdalle: load_one("DESDALLE.SPL").await,
        impact: load_one("IMPACT.SPL").await,
        mort: load_one("MORT.SPL").await,
        victoire: load_one("VICTOIRE.SPL").await,
        gong: load_one("GONG.SPL").await,
        touchdef: load_one("TOUCHDEF.SPL").await,
        chute: load_one("CHUTE.SPL").await,
    }
}

async fn load_one(name: &str) -> Option<Sound> {
    let path = asset_dir().join(name);
    let raw = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!(
                "disc-app: audio: {} unavailable ({err}); this cue will be silent",
                path.display()
            );
            return None;
        }
    };
    let wav = spl_to_wav16(&raw, RATE_HZ);
    match load_sound_from_bytes(&wav).await {
        Ok(sound) => Some(sound),
        Err(err) => {
            eprintln!(
                "disc-app: audio: {name} failed to decode ({err:?}); this cue will be silent"
            );
            None
        }
    }
}

/// Wrap headerless signed 8-bit mono PCM as a 16-bit-per-sample mono WAV
/// file at `rate` Hz, in memory. Mirrors
/// `crates/disc-tools/src/bin/dscfs.rs`'s `pcm8_to_wav16` byte for byte
/// (same scale-up, same chunk layout) -- duplicated rather than imported so
/// `disc-app` does not have to pull in `disc-tools`' dependency tree (clap
/// etc.) for one WAV header, matching this crate's own "the only dependency"
/// policy in `Cargo.toml`.
fn spl_to_wav16(samples: &[u8], rate: u32) -> Vec<u8> {
    let mut pcm16 = Vec::with_capacity(samples.len() * 2);
    for &b in samples {
        let s16 = i16::from(b as i8) * 256;
        pcm16.extend_from_slice(&s16.to_le_bytes());
    }

    const BITS_PER_SAMPLE: u16 = 16;
    const CHANNELS: u16 = 1;
    let block_align: u16 = CHANNELS * BITS_PER_SAMPLE / 8;
    let byte_rate: u32 = rate * u32::from(block_align);
    #[allow(clippy::cast_possible_truncation)]
    let data_len = pcm16.len() as u32;

    let mut buf = Vec::with_capacity(44 + pcm16.len());
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&CHANNELS.to_le_bytes());
    buf.extend_from_slice(&rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    buf.extend_from_slice(&pcm16);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The WAV container this module builds must be byte-identical to
    /// `dscfs`'s own `pcm8_to_wav16` for the same input -- both exist so
    /// `disc-app` never shells out to `dscfs`, but they must still agree.
    #[test]
    fn wav_header_is_well_formed_and_scale_up_is_exact() {
        let samples = [0x00_u8, 0x01, 0xFF, 0x80, 0x7F];
        let wav = spl_to_wav16(&samples, 8000);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1, "PCM format tag");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1, "mono");
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            8000
        );
        assert_eq!(
            u16::from_le_bytes([wav[34], wav[35]]),
            16,
            "bits per sample"
        );
        assert_eq!(&wav[36..40], b"data");

        let data_len = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;
        assert_eq!(data_len, samples.len() * 2);
        let pcm = &wav[44..44 + data_len];
        // 0x00 -> 0, 0x01 -> 256, 0xFF (-1) -> -256, 0x80 (-128) -> -32768,
        // 0x7F (127) -> 32512 -- exact *256, no clipping or rounding.
        let want: [i16; 5] = [0, 256, -256, -32768, 32512];
        for (i, &w) in want.iter().enumerate() {
            let got = i16::from_le_bytes([pcm[i * 2], pcm[i * 2 + 1]]);
            assert_eq!(got, w, "sample {i}");
        }
    }

    /// A cue with nothing loaded must not panic -- the whole point of
    /// `Option<Sound>` fields is a clone without `assets/` still runs.
    #[test]
    fn playing_an_unloaded_cue_is_a_silent_no_op() {
        let sfx = Sfx::default();
        for cue in [
            Cue::Serve,
            Cue::Block,
            Cue::TileDestroyed,
            Cue::Impact,
            Cue::Death,
            Cue::Win,
            Cue::Round,
            Cue::DefendedHit,
            Cue::Fall,
        ] {
            sfx.play(cue); // must not panic
        }
    }
}

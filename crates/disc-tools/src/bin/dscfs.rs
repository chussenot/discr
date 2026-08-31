//! `dscfs` -- inspect and extract the Loriciel custom filesystem embedded in
//! `DSC` (the flattened original floppy image carried by the PP hard-disk
//! adaptation, and the original floppy itself).
//!
//! Bead `discr-rxx.4`. Format reference: `docs/loriciel-formats.md` §3
//! (directory layout), §4 (embedded file formats) and §6 (in-repo
//! verification, including the DISC.ALL/PROGRAM.HA aliasing correction).
//!
//! # Directory format
//!
//! 32-byte big-endian records starting at byte offset `0x200` (track 0,
//! sector 2), one per file, terminated by the first record whose first byte
//! is not ASCII-graphic (there is no explicit entry count on disk):
//!
//! ```text
//! +0x00  char name[14]   ASCII, NUL-padded (8.3 names)
//! +0x0E  u8   pad
//! +0x0F  u8   flag        0 = LAUNCHER.HA, DISC.ALL, *.NSQ; 1 = everything else
//! +0x10  u16  start_track
//! +0x12  u16  start_sector   (1-based)
//! +0x14  u16  sector_count
//! +0x16  u32  byte_size
//! +0x1A  u8   pad[6]
//! ```
//!
//! Linear byte offset of a file: `((start_track * 10) + (start_sector - 1)) * 512`.
//!
//! # Aliasing (§6 correction)
//!
//! The filesystem is not disjoint: `DISC.ALL`'s span legally *contains*
//! `PROGRAM.HA`'s. That overlap is a feature of this filesystem (`DISC.ALL`
//! is an aliasing master entry), not corruption -- `verify` reports it as
//! informative output rather than failing on it.
//!
//! # Error handling
//!
//! Every malformed record ([`DirError`]) is reported with its record index
//! and never panics; parsing simply stops (not an error) at the first
//! non-directory byte, which is how the on-disk format marks the end of the
//! 34-entry table.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Byte offset of the directory's first record (track 0, sector 2).
const DIR_OFFSET: usize = 0x200;
/// Size of one directory record.
const RECORD_SIZE: usize = 32;
/// Bytes per sector.
const SECTOR_SIZE: usize = 512;
/// Sectors per logical track (`docs/loriciel-formats.md` §3).
const SECTORS_PER_TRACK: usize = 10;

/// The three offsets/sizes `docs/loriciel-formats.md` §6 verified in this
/// repository against the primary bytes. `verify` re-asserts them on every
/// run so a future change to the parser or to the checked-in image is caught
/// immediately rather than silently drifting from the documented reference.
const ANCHORS: [(&str, usize, u32); 3] = [
    ("LAUNCHER.HA", 0x1400, 8394),
    ("DISC.ALL", 0x3600, 316786),
    ("50.NSQ", 0x50C00, 69428),
];

/// Filename stem (sans `.SPL`) to disc-core event name, `docs/loriciel-formats.md`
/// §4 plus the bead's mapping. `DIC13` and `VITRE15K` have no confirmed event
/// yet and are documented here as unknown rather than guessed.
const EVENT_MAP: &[(&str, &str)] = &[
    ("CHUTE", "fall"),
    ("DESDALLE", "tile_destroyed"),
    ("MORT", "death"),
    ("PARADE", "block"),
    ("VICTOIRE", "win"),
    ("GONG", "round_gong"),
    ("IMPACT", "disc_impact"),
    ("TOUCHDEF", "hit_defended"),
    ("LAUNCH", "serve"),
    ("DIC13", "unknown (undocumented)"),
    ("VITRE15K", "unknown (undocumented)"),
];

#[derive(Parser)]
#[command(
    name = "dscfs",
    about = "Inspect and extract the Loriciel custom filesystem embedded in DSC."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the directory: name, flag, track/sector, offset, sectors, bytes.
    Ls {
        /// The DSC image (or any file carrying this filesystem at 0x200).
        image: PathBuf,
    },
    /// Extract one or more named entries, or every entry with --all.
    Extract {
        /// The DSC image to read from.
        image: PathBuf,
        /// Extract every directory entry, ignoring NAMES.
        #[arg(long)]
        all: bool,
        /// Directory to write extracted files into (created if missing).
        outdir: PathBuf,
        /// Entry names to extract, e.g. LAUNCHER.HA DISC.ALL. Ignored with --all.
        names: Vec<String>,
    },
    /// Bounds-check every record, print the span map (overlaps included as
    /// informative output -- DISC.ALL's aliasing of PROGRAM.HA is expected,
    /// see docs/loriciel-formats.md §6), and assert the three §6 anchors.
    Verify {
        /// The DSC image to check.
        image: PathBuf,
    },
    /// Decode every *.SPL entry (headerless signed 8-bit mono PCM) to a
    /// 16-bit WAV file.
    ///
    /// The --rate given here is a playback default, not a recovered fact:
    /// the game's true replay rate comes from its own timer setup, which
    /// this tool does not reconstruct. 8000 Hz is a reasonable ST sample-
    /// replay guess, nothing more.
    Samples {
        /// The DSC image to read samples from.
        image: PathBuf,
        /// Directory to write .wav files into (created if missing).
        outdir: PathBuf,
        /// Playback sample rate baked into the WAV header, in Hz.
        #[arg(long, default_value_t = 8000)]
        rate: u32,
    },
}

/// One parsed directory record, plus its derived linear span in the image.
#[derive(Debug, Clone)]
struct DirEntry {
    name: String,
    flag: u8,
    start_track: u16,
    start_sector: u16,
    sector_count: u16,
    byte_size: u32,
}

impl DirEntry {
    /// Linear byte offset: `((start_track * 10) + (start_sector - 1)) * 512`.
    fn offset(&self) -> usize {
        ((self.start_track as usize) * SECTORS_PER_TRACK + (self.start_sector as usize - 1))
            * SECTOR_SIZE
    }

    /// Exclusive end of this entry's span (`offset() + byte_size`).
    fn end(&self) -> usize {
        self.offset() + self.byte_size as usize
    }
}

/// A malformed directory record, reported with its index rather than causing
/// a panic. `parse_directory` stops at the first one.
#[derive(Debug)]
enum DirError {
    /// `byte_size` exceeds what `sector_count * 512` allocates.
    OversizedByteSize {
        index: usize,
        name: String,
        byte_size: u32,
        allocated: u32,
    },
    /// The record's computed span runs past the end of the image.
    SpanBeyondFile {
        index: usize,
        name: String,
        start: usize,
        end: usize,
        file_len: usize,
    },
    /// `start_sector` is 0, which is invalid under the 1-based convention.
    ZeroStartSector { index: usize, name: String },
    /// The name field has non-zero bytes after its first NUL instead of
    /// clean padding -- garbage where the format promises NUL fill.
    DirtyNamePadding { index: usize, raw: [u8; 14] },
}

impl fmt::Display for DirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DirError::OversizedByteSize {
                index,
                name,
                byte_size,
                allocated,
            } => write!(
                f,
                "record {index} ({name:?}): byte_size {byte_size} exceeds its \
                 sector allocation of {allocated} bytes"
            ),
            DirError::SpanBeyondFile {
                index,
                name,
                start,
                end,
                file_len,
            } => write!(
                f,
                "record {index} ({name:?}): span 0x{start:x}..0x{end:x} runs past \
                 the end of the image (0x{file_len:x} bytes)"
            ),
            DirError::ZeroStartSector { index, name } => write!(
                f,
                "record {index} ({name:?}): start_sector is 0, but sectors are 1-based"
            ),
            DirError::DirtyNamePadding { index, raw } => write!(
                f,
                "record {index}: name field {raw:02x?} has non-zero bytes after its \
                 NUL terminator"
            ),
        }
    }
}

impl std::error::Error for DirError {}

/// Parse the directory at [`DIR_OFFSET`], stopping (not erroring) at the
/// first record whose first byte is not ASCII-graphic -- that is how the
/// on-disk format marks the end of the table, there being no explicit count.
///
/// Returns the first [`DirError`] encountered rather than ever panicking, so
/// a corrupt or truncated image is always reported as data, not a crash.
fn parse_directory(data: &[u8]) -> Result<Vec<DirEntry>, DirError> {
    let mut entries = Vec::new();
    let mut index = 0;
    loop {
        let off = DIR_OFFSET + index * RECORD_SIZE;
        let Some(raw) = data.get(off..off + RECORD_SIZE) else {
            break;
        };
        if !raw[0].is_ascii_graphic() {
            break;
        }

        let name_bytes: [u8; 14] = raw[0..14].try_into().expect("slice of len 14");
        let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(14);
        if name_bytes[name_end..].iter().any(|&b| b != 0) {
            return Err(DirError::DirtyNamePadding {
                index,
                raw: name_bytes,
            });
        }
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();

        let flag = raw[15];
        let start_track = u16::from_be_bytes([raw[16], raw[17]]);
        let start_sector = u16::from_be_bytes([raw[18], raw[19]]);
        let sector_count = u16::from_be_bytes([raw[20], raw[21]]);
        let byte_size = u32::from_be_bytes([raw[22], raw[23], raw[24], raw[25]]);

        if start_sector == 0 {
            return Err(DirError::ZeroStartSector { index, name });
        }

        let allocated = sector_count as u32 * SECTOR_SIZE as u32;
        if byte_size > allocated {
            return Err(DirError::OversizedByteSize {
                index,
                name,
                byte_size,
                allocated,
            });
        }

        let entry = DirEntry {
            name,
            flag,
            start_track,
            start_sector,
            sector_count,
            byte_size,
        };
        let start = entry.offset();
        let end = entry.end();
        if end > data.len() {
            return Err(DirError::SpanBeyondFile {
                index,
                name: entry.name,
                start,
                end,
                file_len: data.len(),
            });
        }

        entries.push(entry);
        index += 1;
    }
    Ok(entries)
}

fn read_image(image: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(image).map_err(|e| format!("{}: {e}", image.display()))
}

fn cmd_ls(image: &Path) -> Result<bool, String> {
    let data = read_image(image)?;
    let entries = parse_directory(&data).map_err(|e| e.to_string())?;

    println!(
        "{:<14} {:>4} {:>5} {:>6} {:>10} {:>7} {:>8}",
        "NAME", "FLAG", "TRACK", "SECTOR", "OFFSET", "SECTORS", "BYTES"
    );
    for e in &entries {
        println!(
            "{:<14} {:>4} {:>5} {:>6} 0x{:08x} {:>7} {:>8}",
            e.name,
            e.flag,
            e.start_track,
            e.start_sector,
            e.offset(),
            e.sector_count,
            e.byte_size
        );
    }
    println!("\n{} entries", entries.len());
    Ok(true)
}

fn cmd_extract(image: &Path, all: bool, outdir: &Path, names: &[String]) -> Result<bool, String> {
    let data = read_image(image)?;
    let entries = parse_directory(&data).map_err(|e| e.to_string())?;

    if !all && names.is_empty() {
        return Err("extract: no names given -- pass --all or one or more NAMEs".to_string());
    }

    let targets: Vec<&DirEntry> = if all {
        entries.iter().collect()
    } else {
        let mut targets = Vec::with_capacity(names.len());
        for n in names {
            let entry = entries.iter().find(|e| &e.name == n).ok_or_else(|| {
                format!(
                    "extract: {n:?} not found in {}'s directory",
                    image.display()
                )
            })?;
            targets.push(entry);
        }
        targets
    };

    std::fs::create_dir_all(outdir).map_err(|e| format!("{}: {e}", outdir.display()))?;

    for e in &targets {
        let bytes = data.get(e.offset()..e.end()).ok_or_else(|| {
            format!(
                "extract: {}: span 0x{:x}..0x{:x} is out of bounds for a {}-byte image",
                e.name,
                e.offset(),
                e.end(),
                data.len()
            )
        })?;
        let path = outdir.join(&e.name);
        std::fs::write(&path, bytes).map_err(|err| format!("{}: {err}", path.display()))?;
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    }
    println!(
        "\n{} file(s) extracted to {}",
        targets.len(),
        outdir.display()
    );
    Ok(true)
}

fn cmd_verify(image: &Path) -> Result<bool, String> {
    let data = read_image(image)?;
    let entries = parse_directory(&data).map_err(|e| e.to_string())?;
    println!(
        "{} directory entries parsed and bounds-checked from {}",
        entries.len(),
        image.display()
    );

    let mut spans: Vec<&DirEntry> = entries.iter().collect();
    spans.sort_by_key(|e| e.offset());

    println!("\nSpan map (sorted by offset):");
    for e in &spans {
        println!(
            "  0x{:06x}..0x{:06x} ({:>7} B)  {}",
            e.offset(),
            e.end(),
            e.byte_size,
            e.name
        );
    }

    // All-pairs overlap check (34 entries, so O(n^2) is trivial). Overlap is
    // legal here -- DISC.ALL is an aliasing master entry that legitimately
    // contains PROGRAM.HA's span -- so this is reported, never failed on.
    let mut overlaps = Vec::new();
    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            let (a, b) = (&entries[i], &entries[j]);
            let (s0, e0) = (a.offset(), a.end());
            let (s1, e1) = (b.offset(), b.end());
            if s0 < e1 && s1 < e0 {
                overlaps.push((a.name.clone(), b.name.clone(), s0.max(s1), e0.min(e1)));
            }
        }
    }
    if overlaps.is_empty() {
        println!("\nNo overlaps.");
    } else {
        println!(
            "\n{} overlap(s) -- informative, not an error: this filesystem allows an \
             aliasing master entry (DISC.ALL) to legally contain other files' spans. \
             See docs/loriciel-formats.md §6.",
            overlaps.len()
        );
        for (a, b, start, end) in &overlaps {
            println!("  {a} and {b} share 0x{start:06x}..0x{end:06x}");
        }
    }

    println!("\nAnchors (docs/loriciel-formats.md §6):");
    let mut ok = true;
    for (name, want_offset, want_size) in ANCHORS {
        ok &= match entries.iter().find(|e| e.name == name) {
            Some(e) if e.offset() == want_offset && e.byte_size == want_size => {
                println!("  OK       {name} @ 0x{want_offset:x} / {want_size} bytes");
                true
            }
            Some(e) => {
                println!(
                    "  MISMATCH {name}: expected 0x{want_offset:x}/{want_size}, \
                     got 0x{:x}/{}",
                    e.offset(),
                    e.byte_size
                );
                false
            }
            None => {
                println!("  MISSING  {name}: not found in directory");
                false
            }
        };
    }

    Ok(ok)
}

/// Encode headerless signed 8-bit mono PCM as a 16-bit-per-sample mono WAV
/// file at `rate` Hz. Each input byte is a signed 8-bit sample, scaled up to
/// 16 bits (`sample * 256`) rather than truncated or reinterpreted, so the
/// waveform shape is preserved exactly.
fn pcm8_to_wav16(samples: &[u8], rate: u32) -> Vec<u8> {
    let mut pcm16 = Vec::with_capacity(samples.len() * 2);
    for &b in samples {
        let s16 = i16::from(b as i8) * 256;
        pcm16.extend_from_slice(&s16.to_le_bytes());
    }

    const BITS_PER_SAMPLE: u16 = 16;
    const CHANNELS: u16 = 1;
    let block_align: u16 = CHANNELS * BITS_PER_SAMPLE / 8;
    let byte_rate: u32 = rate * u32::from(block_align);
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

/// The disc-core event a `.SPL` filename stem maps to, or `"unmapped"` for a
/// sample this tool has never seen before.
fn event_for(stem: &str) -> &'static str {
    EVENT_MAP
        .iter()
        .find(|(k, _)| *k == stem)
        .map_or("unmapped", |(_, v)| v)
}

fn cmd_samples(image: &Path, outdir: &Path, rate: u32) -> Result<bool, String> {
    let data = read_image(image)?;
    let entries = parse_directory(&data).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(outdir).map_err(|e| format!("{}: {e}", outdir.display()))?;

    println!("Event mapping (filename stem -> disc-core event):");
    for (stem, event) in EVENT_MAP {
        println!("  {stem:<10} -> {event}");
    }
    println!(
        "\nNOTE: {rate} Hz is a playback default, not a recovered fact -- the game's true \
         replay rate comes from its own timer setup, which this tool does not reconstruct.\n"
    );

    let mut count = 0usize;
    for e in &entries {
        if !e.name.to_ascii_uppercase().ends_with(".SPL") {
            continue;
        }
        let stem = &e.name[..e.name.len() - 4];
        let bytes = data
            .get(e.offset()..e.end())
            .ok_or_else(|| format!("samples: {}: span out of bounds", e.name))?;
        let wav = pcm8_to_wav16(bytes, rate);
        let path = outdir.join(format!("{stem}.wav"));
        std::fs::write(&path, &wav).map_err(|err| format!("{}: {err}", path.display()))?;

        let duration_s = bytes.len() as f64 / f64::from(rate);
        println!(
            "{:<12} {:<24} {:>7} samples  {duration_s:>6.2}s  -> {}",
            e.name,
            event_for(stem),
            bytes.len(),
            path.display()
        );
        count += 1;
    }
    println!("\n{count} sample(s) decoded to {}", outdir.display());
    Ok(true)
}

fn run(cli: Cli) -> Result<bool, String> {
    match cli.command {
        Command::Ls { image } => cmd_ls(&image),
        Command::Extract {
            image,
            all,
            outdir,
            names,
        } => cmd_extract(&image, all, &outdir, &names),
        Command::Verify { image } => cmd_verify(&image),
        Command::Samples {
            image,
            outdir,
            rate,
        } => cmd_samples(&image, &outdir, rate),
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("dscfs: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `assets/disch/DSC`, read fresh each time (it is a few hundred KB --
    /// not worth `include_bytes!`-ing into the test binary).
    fn dsc_bytes() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/disch/DSC"
        ))
        .expect("assets/disch/DSC is checked into the repo")
    }

    /// docs/loriciel-formats.md §6's three verified anchors.
    #[test]
    fn anchors_match_the_documented_offsets() {
        let data = dsc_bytes();
        let entries = parse_directory(&data).expect("real directory parses cleanly");
        for (name, want_offset, want_size) in ANCHORS {
            let e = entries
                .iter()
                .find(|e| e.name == name)
                .unwrap_or_else(|| panic!("{name} missing from directory"));
            assert_eq!(e.offset(), want_offset, "{name} offset");
            assert_eq!(e.byte_size, want_size, "{name} byte_size");
        }
    }

    /// §3/§6: all 34 directory entries re-extract byte-identical to the
    /// pre-extracted copies checked into `assets/original/`.
    #[test]
    fn reextraction_matches_original_byte_for_byte() {
        let data = dsc_bytes();
        let entries = parse_directory(&data).expect("real directory parses cleanly");
        let original_dir = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/original"
        ));

        for e in &entries {
            let extracted = &data[e.offset()..e.end()];
            let original_path = original_dir.join(&e.name);
            let original = std::fs::read(&original_path)
                .unwrap_or_else(|err| panic!("{}: {err}", original_path.display()));
            assert_eq!(
                extracted,
                original.as_slice(),
                "{} differs from assets/original",
                e.name
            );
        }
        assert_eq!(entries.len(), 34, "expected all 34 directory entries");
    }

    /// A record whose `byte_size` exceeds its sector allocation is a typed,
    /// indexed error -- never a panic.
    #[test]
    fn oversized_byte_size_errors_without_panicking() {
        let mut data = vec![0u8; DIR_OFFSET + RECORD_SIZE];
        let rec = &mut data[DIR_OFFSET..DIR_OFFSET + RECORD_SIZE];
        rec[0..14].copy_from_slice(b"BAD.DAT\0\0\0\0\0\0\0");
        rec[15] = 0; // flag
        rec[16..18].copy_from_slice(&1u16.to_be_bytes()); // start_track
        rec[18..20].copy_from_slice(&1u16.to_be_bytes()); // start_sector (1-based)
        rec[20..22].copy_from_slice(&1u16.to_be_bytes()); // sector_count = 1 -> 512 B allocated
        rec[22..26].copy_from_slice(&1000u32.to_be_bytes()); // byte_size 1000 > 512

        match parse_directory(&data) {
            Err(DirError::OversizedByteSize { index, .. }) => assert_eq!(index, 0),
            other => panic!("expected OversizedByteSize at index 0, got {other:?}"),
        }
    }

    /// A record whose span runs past the end of the image is also a typed,
    /// indexed error.
    #[test]
    fn span_beyond_file_errors_without_panicking() {
        let mut data = vec![0u8; DIR_OFFSET + RECORD_SIZE];
        let rec = &mut data[DIR_OFFSET..DIR_OFFSET + RECORD_SIZE];
        rec[0..14].copy_from_slice(b"BAD.DAT\0\0\0\0\0\0\0");
        rec[16..18].copy_from_slice(&1u16.to_be_bytes()); // start_track
        rec[18..20].copy_from_slice(&1u16.to_be_bytes()); // start_sector
        rec[20..22].copy_from_slice(&2u16.to_be_bytes()); // sector_count = 2 -> 1024 B allocated
        rec[22..26].copy_from_slice(&900u32.to_be_bytes()); // byte_size 900 <= 1024, but the image is tiny

        match parse_directory(&data) {
            Err(DirError::SpanBeyondFile { index, .. }) => assert_eq!(index, 0),
            other => panic!("expected SpanBeyondFile at index 0, got {other:?}"),
        }
    }
}

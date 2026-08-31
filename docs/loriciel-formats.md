# Disc (Loriciel, 1990) — Archive & File Format Reference

Analysis of `disc.zip` (AtariMania preservation dump) and `DISCH.ZIP`
(Peter Putnik hard-disk adaptation, 2010–2012). All offsets verified
against the uploaded binaries. 68000 conventions: big-endian throughout.

---

## 1. disc.zip — preservation dump (one physical disk, four representations)

| File | Format | Role |
|---|---|---|
| `Disc - Loriciel/track*.{0,1}.hxcstream` | HxC stream (`CHKH` chunks, embedded metadata) | Raw flux capture, 83 cylinders × 2 sides, source of truth |
| `Disc - Loriciel.scp` | SuperCard Pro v0.9, disktype 0x15, 3 revolutions, tracks 0–165, flags 0x83 (index-synced, 96 tpi, footer) | Flux archive converted from the stream files |
| `Disc - Loriciel.hfe` | HXCPICFE rev 0, 83 trk × 2 sides, ISO/IBM MFM, 251 kbit/s | Decoded bitstream for Gotek/HxC hardware emulators |
| `Disc - Loriciel.stx` | Pasti `RSY\0` v3, tool 0x00AF, 166 track records, rev 2 | Sector-level image with protection metadata — the one Hatari reads |
| `Disc - Loriciel.png` | libhxcfe v2.15.6.1 track view | Side 0: 860 sectors, **70 bad** (protection). Side 1: 790 sectors, 0 bad |

Pipeline: `hxcstream (capture) → scp (flux archive) → hfe (bitstream) + stx (sector image)`.
The 70 "bad" sectors concentrated on side 0 are the Loriciel protection
scheme; only STX preserves their fuzzy/timing behavior for software emulation.

## 2. DISCH.ZIP — PP hard-disk adaptation

| File | Format | Role |
|---|---|---|
| `RUNME.TOS` | GEMDOS PRG (magic `0x601A`), text=230 dat=6448 bss=71750; contains a Pack-Ice (`Ice!`) packed payload and references `DISHA3.TOS` | Launcher: installs HAGA, loads DSC, patches, jumps to game |
| `HAGA` | Raw data; jump/offset table + code | PP's "HArddisk Gaming Atari" runtime library (12 functions: disk I/O redirection, exit-to-desktop, gamestate snapshots) |
| `DISC.PCH` | Depacked Photochrome picture (per LOG.TXT: TGA → PCS → PCH). 102,470 B ≈ 2 fields × (32,000 screen + 19,200 per-scanline palette) + 70 B header | Cover/title picture shown by the loader |
| `FILES/D15H5.FIC` | Loriciel container, magic `C3`; BE dwords at +2: 0x981F, 0xDD5E, 0x611C; name field `D15H5` at +16 | Auxiliary Loriciel data file (packed; 38,975 B on disk, size fields suggest ~56 KB unpacked) |
| `FILES/DSC` | Flattened image of the original floppy's **custom Loriciel filesystem** (see §3) | The entire original game: bootstrap + directory + 34 files |
| `README.TXT`, `LOG.TXT`, `INSTRUCT.TXT` | ASCII | PP's notes (toolchain, HAGA design) + full gameplay manual |

`DSC` begins with raw supervisor-mode 68000 code (`46FC 2700` = `move #$2700,SR`)
— the boot/bootstrap sector — not a GEMDOS executable. Load as raw binary
in Ghidra.

## 3. Loriciel on-disk filesystem (inside DSC, and on the original floppy)

- Geometry: **10 sectors/track, 512 B/sector, sectors 1-based**,
  logical track = cylinder×2 + side. Files are contiguous.
- Directory at sector offset `0x200` (track 0, sector 2). 34 entries.
- Record layout, 32 bytes, big-endian:

```
+0x00  char name[14]   ASCII, NUL-padded (8.3 names)
+0x0E  u8   pad
+0x0F  u8   flag        0 = LAUNCHER.HA, DISC.ALL, *.NSQ; 1 = everything else (semantics unconfirmed)
+0x10  u16  start_track
+0x12  u16  start_sector   (1-based)
+0x14  u16  sector_count
+0x16  u32  byte_size
+0x1A  u8   pad[6]
```

- Linear byte offset of a file: `((start_track * 10) + (start_sector - 1)) * 512`.
- Verified: consecutive entries are exactly contiguous under this formula
  (e.g. LAUNCHER.HA ends where DISC.ALL begins; DISC.ALL ends where 50.NSQ begins).

## 4. Embedded file formats (34 files, all extracted)

| Pattern | Format | Notes |
|---|---|---|
| `LAUNCHER.HA` | `HABS` header: magic(4) + u32×6 = 28 B; load addr `0x1000`, code len `0x1F72`; entry `BRA.W` at +0x1C | Loriciel absolute executable |
| `PROGRAM.HA` | No HABS magic; high-entropy signed bytes | Main game program, packed or encrypted (80,340 B) |
| `*.NSQ` | `NSEQ` + u8 count (`50.NSQ`: count=0x32=50; `LORI.NSQ`: count=0x24=36) + offset table | Loriciel animation sequence (intro/logo). Offset encoding not fully decoded |
| `*.SPL` | Headerless signed 8-bit PCM | Sound samples; French names map to game events (CHUTE=fall, DESDALLE=tile destroyed, MORT=death, PARADE=block, VICTOIRE, GONG…) |
| `CONVERTX/Y.DAT` | Signed-byte LUTs, 2094 / 1071 B | Coordinate/projection conversion tables (X ≈ 2× Y — screen aspect) |
| `HEADS.DAT` | Exactly 256 B, periodic signed 8-byte rows | Small LUT — likely sprite hotspot/bobbing offsets (speculative) |
| `DECOR*.DAT`, `DALLES01.DAT`, `PLAYER01.DAT`, `ENEMY01.DAT`, `VIC.DAT`, etc. | Signed-byte / delta-looking data | Graphics, likely Loriciel-packed; decoder lives in PROGRAM.HA / bootstrap |
| `DISC.ALL` | u16 count=18(?) then u32 table | Master data/level index; partially decoded |

Directory total: 758,847 B of file data in a 773,120 B (0xBCC00 = 1,510
sectors = 151 logical tracks) container; the difference is boot/directory
area plus sector-rounding slack.

## 5. Gameplay facts from INSTRUCT.TXT relevant to disc-core

- Arena: per player, 2×4 floor tiles + 2×4 back-wall tiles, wall tile N
  destroys floor tile N.
- Wall tiles carry a hit-count rendered as a shape cycle:
  pentagon(5) → square(4) → triangle(3) → equals(2) → circle(1) → gone.
  Higher levels start deeper in the cycle.
- Lemniscate ("sideways 8") tiles are indestructible.
- Win conditions: drain life meter, or destroy all standable tiles.

These independently corroborate the reverse-engineered tile-HP model
(per-tile HP byte, per-disc damage at record +0x16, writer $A34C).

---

## 6. In-repo placement and verification (2026-08-31, this repository)

The files live in the repository:

- `assets/original/` — the 34 files of the Loriciel filesystem, extracted
  from `DSC`.
- `assets/disch/` — the PP adaptation: `DSC` itself, `HAGA`, `RUNME.TOS`,
  `DISC.PCH`, `D15H5.FIC`, and PP's `README.TXT`/`LOG.TXT`/`INSTRUCT.TXT`.

Verified in this repository against the primary bytes:

- The §3 directory layout re-extracts **all 34 files byte-identical** from
  `assets/disch/DSC` (parse at `0x200`, offset formula as documented).
- `LAUNCHER.HA`'s `HABS` header reads exactly as §4 says: load `0x1000`,
  code len `0x1F72`, entry `BRA.W` (`6000 1BEC`) at `+0x1C`.
- `50.NSQ` begins `NSEQ` + `0x32` (= 50), as documented.
- **Correction to §3's contiguity claim**: the filesystem is NOT disjoint.
  `PROGRAM.HA` (`trk 4 sec 3` → `0x5400`–`0x18E00`) lies entirely INSIDE
  `DISC.ALL`'s span (`trk 2 sec 8`, 619 sectors → `0x3600`–`0x50C00`).
  `DISC.ALL` is an aliasing master entry covering other files' regions —
  consistent with its name and its own internal u32 table (§4). Neighbour
  contiguity holds for the verified examples (`LAUNCHER.HA`→`DISC.ALL`→
  `50.NSQ` by span end), but a general verifier must model overlap as
  legal, not an error. Owned by the `dscfs` tool's `verify` subcommand.
- **The aliasing is wider than one pair.** `dscfs verify`'s all-pairs check
  (run against `assets/disch/DSC`) finds `DISC.ALL`'s span containing or
  overlapping 29 of the intervening entries, not just `PROGRAM.HA` — every
  file between `PROGRAM.HA` and `ENEMY01.DAT` sits inside or across its
  `0x3600`–`0x50B72` span, and `50.NSQ` in turn overlaps the tail of
  `ENEMY01.DAT` and all of `VIC.DAT`. `DISC.ALL` reads as a master index
  over the whole packed-data region, not a special case around one file.
  Full span map in `reports/part13-dscfs.md`.
- **Tool**: `cargo run -p disc-tools --bin dscfs -- {ls,extract,verify,samples} <DSC>`
  parses this directory, extracts entries, bounds-checks + reports the span
  map above, and decodes `*.SPL` to WAV. `crates/disc-tools/src/bin/dscfs.rs`.

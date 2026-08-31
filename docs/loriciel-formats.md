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
+0x0F  u8   flag        0 = LAUNCHER.HA, DISC.ALL, *.NSQ; 1 = everything else
+0x10  u16  start_track
+0x12  u16  start_sector   (1-based)
+0x14  u16  sector_count
+0x16  u32  byte_size
+0x1A  u8   pad[6]
```

- Linear byte offset of a file: `((start_track * 10) + (start_sector - 1)) * 512`.
- Verified: consecutive entries are exactly contiguous under this formula
  (e.g. LAUNCHER.HA ends where DISC.ALL begins; DISC.ALL ends where 50.NSQ begins).

**Flag byte, bounded (2026-08-31, discr-rxx.6).** The `flag=0` class
(LAUNCHER.HA, DISC.ALL, `*.NSQ`) is loaded through a mechanism proven,
address-by-address, to never consult this directory at all: the 512-byte
physical boot sector (`DSC` offset `0`) loads its second stage via
hardcoded, literal sector counts with no name/flag reference (full
`capstone` disassembly, `$0`-`$1fe`), LAUNCHER.HA sits at the fixed
track1/sector1 position that bootstrap seeks to, and LAUNCHER.HA's own
resident code plays `*.NSQ` by walking the `.NSQ` file's *own* embedded
offset table (§4) — a different structure from this directory. `flag=0`
files are therefore the disc's bootstrap dependencies, loaded by hardcoded
position before any named-resource lookup exists; `flag=1` covers every
other (ordinary, presumably name/index-looked-up) resource. The specific
instruction that reads a record's `+0x0F` byte and branches on it was not
located — not in LAUNCHER.HA's full disassembly, not in either disk image's
boot region (PP's `DSC` zeroes that span; the `.st` crack release's
equivalent span is the crack group's own greeting-text loader, not
Loriciel's code). The remaining candidate is PROGRAM.HA, packed and not
statically disassemblable (see depack's reports). Full evidence chain:
`reports/part13-formats.md` §3.

## 4. Embedded file formats (34 files, all extracted)

| Pattern | Format | Notes |
|---|---|---|
| `LAUNCHER.HA` | `HABS` header: magic(4) + u32×6 = 28 B; load addr `0x1000`, code len `0x1F72`; entry `BRA.W` at +0x1C | Loriciel absolute executable |
| `PROGRAM.HA` | No HABS magic; high-entropy signed bytes | Main game program, packed or encrypted (80,340 B) |
| `*.NSQ` | `NSEQ` + u8 count (`50.NSQ`: count=0x32=50; `LORI.NSQ`: count=0x24=36) + offset table | Loriciel animation sequence (intro/logo). Fully decoded, see below |
| `*.SPL` | Headerless signed 8-bit PCM | Sound samples; French names map to game events (CHUTE=fall, DESDALLE=tile destroyed, MORT=death, PARADE=block, VICTOIRE, GONG…) |
| `CONVERTX/Y.DAT` | Signed-byte LUTs, 2094 / 1071 B | Coordinate/projection conversion tables (X ≈ 2× Y — screen aspect) |
| `HEADS.DAT` | Exactly 256 B, periodic signed 8-byte rows | Small LUT — likely sprite hotspot/bobbing offsets (speculative) |
| `DECOR*.DAT`, `DALLES01.DAT`, `PLAYER01.DAT`, `ENEMY01.DAT`, `VIC.DAT`, etc. | Signed-byte / delta-looking data | Graphics, likely Loriciel-packed; decoder lives in PROGRAM.HA / bootstrap |
| `DISC.ALL` | u16 field + 4×u32 header, then nested 0xFFFF-terminated sub-tables (see below) | Master index over its own packed-payload region — not a flat index of the 29 aliased files (mechanically tested and rejected, see below) |

Directory total: 758,847 B of file data in a 773,120 B (0xBCC00 = 1,510
sectors = 151 logical tracks) container; the difference is boot/directory
area plus sector-rounding slack.

### `*.NSQ` animation-sequence format, decoded end-to-end (discr-rxx.6)

`NSEQ` magic (4 B) + `u8 count` at `+4` (real per-frame entry count — no
scaling) + a table of `count` 3-byte, byte-swapped entries starting at `+8`:
`off = (b0<<16) | (b2<<8) | b1` for bytes `(b0,b1,b2)` of each entry (the
same "high byte, then a byte-swapped 16-bit word" idiom used throughout this
format). Confirmed against `LAUNCHER.HA`'s disassembly (`$1048`-`$1072`,
`mulu.w #$3,d0` fixes the stride).

Per frame index, `off` points at a 32-byte record. If the record's first
16-bit word (byte-swapped) masked with `$f800` equals `$f800`, the record is
skipped structurally (no descrambling) and the frame's row-offset table
starts *at* `off` itself; otherwise the 32 bytes are bit-unscrambled via
`LAUNCHER.HA`'s `$126a` into a fixed shared destination (animation-wide
state, not needed for byte accounting) and the row-offset table starts at
`off + 0x20`. From there, 16-bit words (same byte-swap) are scanned until a
literal `0xFFFF` terminator; if the very first word already *is* `0xFFFF`,
the frame is legitimately empty (a "hold" frame — `LAUNCHER.HA`'s `$10ac`/
`$10b2`, a clean early return, not an error). Otherwise, immediately after
the terminator, the control-word stream begins — already documented and
implemented in `scripts/loriciel_depack.py`'s `decode_nsq_control_stream()`
(`$10c4`-`$1170`), unchanged by this pass.

Validated end-to-end against the real disk image: **50/50** of `50.NSQ`'s
frames decode cleanly (a clean control-word terminator or a legitimate empty
frame) and **36/36** of `LORI.NSQ`'s (27 clean decodes + 9 legitimate empty
frames) — zero unaccounted frames, zero decode failures, in either file.
Several of `50.NSQ`'s later frames' control-word streams read a few dozen to
~340 bytes past `50.NSQ`'s own declared end, directly into the following
file on disk (`LORI.NSQ`'s header, confirmed by literal `NSEQ` magic match)
— this is the original session's one unresolved "10th offset": not a
different record type or a second table, just `LAUNCHER.HA` never
bounds-checking a frame's control stream against its own file's
`byte_size`, harmless on real hardware since the whole region loads into one
contiguous buffer. Full evidence chain and validation script:
`reports/part13-formats.md` §1.

### `DISC.ALL`'s internal header (discr-rxx.6)

The "u16 count + u32 table indexes the 29 aliased files" hypothesis from §6
is **rejected**, tested mechanically: none of the header's values (plain,
byte-swapped, absolute-`DSC`, or `DISC.ALL`-relative) land on any of the 29
aliased files' start offsets. What the header *does* encode: `u16` field at
`+0`, then 4×`u32` at `+2..+18` — three of those four (`764`, `918`, `1648`)
are each the file offset immediately following a literal `0xFFFF` word (the
same terminator convention `*.NSQ`'s row table uses); the fourth (`138`) is
exactly where a fixed 20-entry, 6-byte-stride sub-table starting at `+18`
ends (`18 + 20*6 = 138`) — that sub-table's `u32` half is a strictly
descending, always-in-bounds sequence of offsets into `DISC.ALL` itself,
with several deltas repeating exactly (paired chunk boundaries, not random
data). Reads as `DISC.ALL`'s own internal packed-payload index, not a
directory of the 29 aliased files. A descending offset table is also
exactly the shape a *backward*-processing decompressor would need — a
concrete new lead for `reports/part13-depack.md`'s still-unproven
Pack-Ice-class decoder (Finding 3), not chased further this wave. Full
evidence chain: `reports/part13-formats.md` §2.

### The TRUE Ice! containers: a constant offset from the declared directory (discr-6by)

`docs/loriciel-formats.md` ss6 and `reports/part13-codec.md` established that
the directory's own `((start_track*10)+(start_sector-1))*512` formula points
at **decoy** bytes for the packed (flag=1) file class — the live Ice!
depacker's real input was only found by live-capturing RAM and searching the
disk image for the captured header. This wave (discr-6by) found the general
rule, byte-scanning `assets/disch/DSC` (773,120 B) for every `"Ice!"` magic
occurrence and cross-referencing against the directory:

```
true_offset = declared_offset + 404480        # 404480 B = 790 sectors, DSC only
```

Verified against **all 23** of the directory's flag=1 entries that carry an
`"Ice!"` header at this computed position (every one: header `P` == the
directory's own `byte_size` field, exactly) — and independently confirmed
bit-exact against all four live-Hatari ground-truth proof pairs
(`reports/part13-codec.md`'s DALLES01/PLAYER01/ENEMY01/DECOR00 sha256 table),
reproduced fully offline via `scripts/loriciel_depack.py`'s new
`extract_container()` + `depack_ice()`. Newly decoded this pass, never
before observed loading in any live session: **DECOR01/02/03/04.DAT**, all
four to a valid `32,032`-byte (`32` B palette + `320x200x4bpp` = `32,000` B
screen) ST low-res picture — see `reports/part14-containers.md`.

The 790-sector delta is not arbitrary: `docs/loriciel-formats.md` ss1's own
independently-measured disc geometry is "side 0: 860 sectors, **70 bad**" —
`860 - 70 = 790`. The true-container region begins immediately after all of
side 0's *good* sectors in this flattened image, consistent with PP's
hard-disk conversion having already resolved the bad-sector protection this
disc uses (ss1).

**Entries that don't carry an `"Ice!"` header at the predicted position**
(`PROGRAM.HA`, `DESDALLE.SPL`, `GONG.SPL`, `TOUCHDEF.SPL`, `VITRE15K.SPL`,
`DIC13.SPL`, `HEADS.DAT`) still land on real, structured content there —
not noise. Notably: **`PROGRAM.HA`'s true position** (`DSC $68000`) carries
a proper `HABS` header (magic + `u32`x6: load addr `$8000`, code len
`$D6FC`) with real 68000 code immediately following (`46FC 2700` = `move
#$2700,SR`, the same supervisor-mode-entry idiom `LAUNCHER.HA` and the boot
sector use) — a world away from the decoy bytes at `PROGRAM.HA`'s own
*declared* directory position, which ss4 already flags as "No HABS magic;
high-entropy signed bytes." See `reports/part14-containers.md` for the
discr-zg4 angle this opens (not independently live-confirmed this pass).

**The bigger repo-root `.st` image (819,200 B)** carries the SAME
containers (all four proof-pair headers found there too, at the exact
addresses `reports/part13-codec.md` cites, e.g. `$75600` for DALLES01.DAT)
but at a **per-file variable** delta, not a constant one — because `.st` is
the raw physical floppy dump (protection intact) and DSC is PP's flattened,
already-de-protected conversion. Measured directly: reading a `.st`
container linearly reproduces the DSC ground truth for the first 5,120
bytes (10 sectors = exactly one track, ss3's "10 sectors/track" geometry) of
every 10-sector run, then a 10-sector (5,120 B) gap of foreign bytes
intervenes before the next 10-sector run resumes cleanly — repeating for
the whole container. This is the disc's own documented cylinder-major side
interleave (ss3: "logical track = cylinder×2 + side"): the true containers
live entirely on one side's tracks, and `.st`'s raw physical byte order
alternates that side's 10-sector tracks with the *other* side's 10-sector
tracks in between. `extract_container()` implements the DSC constant-delta
form only; `.st` needs this track-degapping pass, not implemented (DSC
alone already round-trips every proof pair, so no consumer needs it yet).

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

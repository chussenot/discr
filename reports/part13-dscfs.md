# Part 13: `dscfs`, the Loriciel filesystem tool

bd discr-rxx.4. Every claim below is either a verified byte offset (via the
tool run against `assets/disch/DSC`, the real image) or a citation into
`docs/loriciel-formats.md`.

## What the tool asserts

`crates/disc-tools/src/bin/dscfs.rs` parses the 32-byte big-endian directory
records at `0x200` (`docs/loriciel-formats.md` §3) and provides four
subcommands:

- `ls <image>` — table of name / flag / track / sector / offset / sectors /
  bytes for every directory entry.
- `extract <image> [--all] <outdir> [names...]` — writes the exact
  `byte_size` bytes of one or more entries, or every entry with `--all`.
- `verify <image>` — bounds-checks every record, prints the full span map
  sorted by offset, reports every pairwise overlap as informative output
  (never a failure — this filesystem allows aliasing, §6), and asserts the
  three §6 anchor offsets/sizes. Exits non-zero only if bounds-checking fails
  or an anchor mismatches.
- `samples <image> <outdir> [--rate HZ]` — decodes every `*.SPL` entry
  (headerless signed 8-bit mono PCM) to a 16-bit PCM mono WAV at `--rate` Hz
  (default 8000), and prints the filename → disc-core event mapping.

Malformed records (`byte_size` exceeding the sector allocation, a span
running past the end of the image, `start_sector == 0`, or non-zero garbage
after a name's NUL terminator) are typed errors (`DirError`) carrying the
record index — parsing never panics, and a truncated/corrupt image just
yields fewer directory entries or a reported error, not a crash.

The parser stops (not an error) at the first record whose first byte is not
ASCII-graphic, since the on-disk format carries no explicit entry count.

## `verify` output against the real image (`assets/disch/DSC`)

All 34 directory entries parse and bounds-check cleanly. Sorted span map
(offset, size, name) — abbreviated to first/last few and the anchor entries;
the full 34-row table is reproducible with `dscfs ls`:

```
0x001400.. 8394 B  LAUNCHER.HA
0x003600..316786 B  DISC.ALL
0x005400.. 80340 B  PROGRAM.HA
   ... 26 more entries, all inside or across DISC.ALL's span ...
0x050c00.. 69428 B  50.NSQ
0x055600.. 17908 B  VIC.DAT
0x061c00.. 25592 B  LORI.NSQ
```

**Anchors** (`docs/loriciel-formats.md` §6), all OK:

| name | offset | bytes |
|---|---|---|
| LAUNCHER.HA | 0x1400 | 8394 |
| DISC.ALL | 0x3600 | 316786 |
| 50.NSQ | 0x50c00 | 69428 |

**Overlap finding — wider than §6's documented case.** The all-pairs check
finds **31 overlapping pairs**, not the single `DISC.ALL`/`PROGRAM.HA` pair
§6 records:

- `DISC.ALL` (`0x003600`–`0x050b72`) contains or overlaps **29** of the
  intervening entries: `PROGRAM.HA` and every `.DAT`/`.SPL`/`.NSQ`-adjacent
  file up through `ENEMY01.DAT` (whose tail actually runs past `DISC.ALL`'s
  end, so that one is a partial overlap, not full containment — every other
  member of the 29 is fully contained).
- `50.NSQ` (`0x050c00`–`0x061b34`) overlaps the tail of `ENEMY01.DAT` and
  fully contains `VIC.DAT`.

This is consistent with `DISC.ALL` being a master index over the packed-data
region as a whole (§4: "master data/level index"), not a special case
confined to `PROGRAM.HA`. `docs/loriciel-formats.md` §6 now has a line
pointing at this. None of this is a bug: `verify` reports overlaps as
informative and exits 0 when bounds-checking and the anchors pass, which is
what it does here.

Exit code: `0`.

## `extract --all` — 34/34 byte-identical

`dscfs extract assets/disch/DSC --all <dir>` followed by `diff -rq` against
`assets/original/` reports every one of the 34 files identical. This is
pinned as `tests::reextraction_matches_original_byte_for_byte`.

## WAV inventory (`samples`, default 8000 Hz)

11 `*.SPL` entries, each headerless signed 8-bit mono PCM, decoded to 16-bit
PCM mono WAV (`sample * 256`, exact scale-up, no clipping/rounding loss):

| file | disc-core event | samples | duration @ 8 kHz | .wav size |
|---|---|---:|---:|---:|
| CHUTE.SPL | fall | 5858 | 0.73 s | 11760 B |
| DESDALLE.SPL | tile_destroyed | 6468 | 0.81 s | 12980 B |
| MORT.SPL | death | 8577 | 1.07 s | 17198 B |
| PARADE.SPL | block | 1780 | 0.22 s | 3604 B |
| VICTOIRE.SPL | win | 4951 | 0.62 s | 9946 B |
| GONG.SPL | round_gong | 14651 | 1.83 s | 29346 B |
| IMPACT.SPL | disc_impact | 1826 | 0.23 s | 3696 B |
| TOUCHDEF.SPL | hit_defended | 3987 | 0.50 s | 8018 B |
| LAUNCH.SPL | serve | 4501 | 0.56 s | 9046 B |
| DIC13.SPL | unknown (undocumented) | 3843 | 0.48 s | 7730 B |
| VITRE15K.SPL | unknown (undocumented) | 2981 | 0.37 s | 6006 B |

`file(1)` confirms each output as `RIFF ... WAVE audio, Microsoft PCM, 16
bit, mono 8000 Hz`. **Caveat, also printed by `dscfs samples --help` and at
runtime**: 8000 Hz (or whatever `--rate` is given) is a playback default,
not a recovered fact — the game's true sample-replay rate comes from its own
Timer A setup (`docs/disc-notes.md` names `$134 -> $83d2`, "MFP 13, Timer A,
~4.9 kHz PSG sample streamer" for the live-match streamer, but that is not
necessarily the same rate the *offline* `.SPL` assets were authored at), and
this tool does not reconstruct it.

## Tests (`cargo test -p disc-tools --bin dscfs`)

4 tests, all passing:

- `anchors_match_the_documented_offsets` — the three §6 anchors, against the
  real `assets/disch/DSC`.
- `reextraction_matches_original_byte_for_byte` — all 34 entries, byte-exact
  against `assets/original/`.
- `oversized_byte_size_errors_without_panicking` — a hand-built record with
  `byte_size` exceeding its sector allocation returns a typed, indexed error.
- `span_beyond_file_errors_without_panicking` — a hand-built record whose
  span runs past a truncated image returns a typed, indexed error.

## Gates run

```
cargo fmt --check                                                   -- clean
cargo clippy --all-targets -- -D warnings                           -- clean
cargo test                                                           -- 57+1+4+11 passed, 0 failed
cargo run -q -p disc-tools --bin dscfs -- verify assets/disch/DSC   -- exit 0, 31 overlaps (informative), 3/3 anchors OK
cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/golden.ndjson --min-agree 99
                                                                      -- OK: 99 tick(s) matched (untouched)
```

## Files

- `crates/disc-tools/src/bin/dscfs.rs` — the tool.
- `crates/disc-tools/Cargo.toml` — new `[[bin]] dscfs` target.
- `docs/loriciel-formats.md` — §6 gains the widened-overlap finding and a
  pointer to the tool.
- `reports/part13-dscfs.md` — this file.

Not committed here: `assets/`. It is read-only input owned by `orchestrator`,
already committed upstream on `claude/atari-abandonware-download-muie9c` (not
yet merged into this branch at the time of this work). It was recovered
locally (via a path-scoped, uncommitted cherry-pick restore) only so this
branch's own tests and gates could run against the real image; the two
`assets/*/reextraction`/anchor tests need it present on disk at test time,
same as `docs/loriciel-formats.md` §6 already assumed for every other
verification claim in this repo.

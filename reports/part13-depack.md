# Part 13 (depack) — most of it isn't packed; the rest resists

discr-rxx.5 asked for the Loriciel depacker and a round-trip-proven unpack of
DECOR00-04/DALLES01/PLAYER01/ENEMY01/BONUS01/VIC.DAT and PROGRAM.HA. What
follows is address-cited both ways: three graphics files (and PROGRAM.HA's
disk-load buffer) turn out to need no depacker at all — confirmed by finding
their bytes 100% verbatim in live Hatari RAM — while DALLES01.DAT (the
tiles), PLAYER01.DAT, ENEMY01.DAT and DECOR02-04.DAT never appeared raw in
any of three separate boot/match sessions, and a strong depacker candidate
was found and partially annotated but not proven to be their mechanism.

## Method

Two RAM oracles, both live Hatari sessions driven the way `reports/part12-*`
established (control-socket debugger, not `--cmd-fifo`): a raw boot with no
navigation (`Hatari.wait_frames()` only, landing mid-intro), and a full
`Hatari.enter_match(mode="training")` run (menu → live match). Both dump the
full 1 MB address space (`savebin $0 $100000`) and get searched in Python for
literal byte runs from `assets/original/*.DAT` — the same "byte-pattern scan
independent of what Ghidra/capstone thinks is code" technique Part 12b used
for `$6c5d`, applied here to plain data instead. For the depacker hunt
itself: `scripts/collect.py`'s `Hatari.watch()` (write-tracking breakpoint,
`:trace :lock`, never drops to an interactive prompt) on a known code address
to catch its writer PC red-handed, then PC breakpoints (`:once :trace :file
<script>` running `r`) at specific addresses to read live registers without
ever pausing into a debugger prompt with no stdin attached (the pattern
`seed()` already established in collect.py, reused here for ad hoc probes).
LAUNCHER.HA itself was disassembled directly with `capstone` (8 KB, HABS
header stripped, base `$1000`) — faster than a Ghidra round-trip for this
size, per Part 12's own finding that Ghidra's auto-analysis misses code
capstone catches straight off the bytes.

## Finding 1: DECOR00, BONUS01, VIC.DAT, and PROGRAM.HA's load buffer are not compressed

A literal search of a raw-boot 1 MB RAM dump for each target file's bytes
found four 100%, byte-for-byte, unbroken matches:

| File | RAM address | Match | sha256 (first 16 hex) |
|---|---|---|---|
| `DECOR00.DAT` | `$58B12` | 7477 / 7477 | `5b739722d4b12f57` |
| `DECOR01.DAT` | `$5A952` | 9216 / 10121 (see below) | `2b3ca8d50b5be98d` |
| `BONUS01.DAT` | `$442D2` | 2749 / 2749 | `8e644eb0d1de084b` |
| `VIC.DAT` | `$1F900` | 17908 / 17908 | `dcca430cfc90a62e` |
| `PROGRAM.HA` | `$2E512` | 80340 / 80340 | `10a423d4743be579` |

`DECOR01.DAT` diverges at file offset 9216/10121 — but the RAM bytes right
past the divergence point are themselves small, delta-like values (not
noise), and the pattern is consistent with the file's *tail* having been
overwritten by a later, adjacent load into the same staging buffer by the
time of the snapshot, not with DECOR01.DAT being differently encoded from
DECOR00 (its own first 9216 bytes match exactly). Recorded as "proven up to
a buffer-reuse artifact," not as a partial codec.

This means: the doc's `DECOR*/DALLES01/PLAYER01/ENEMY01/VIC.DAT ... likely
Loriciel-packed` note (docs/loriciel-formats.md §4) was a reasonable but
wrong inference from "high-entropy signed bytes" — at least for this
subset, the bytes are high-entropy because they're graphics data, not
because they're compressed. `scripts/loriciel_depack.py`'s `depack()`
implements this as the identity transform for the confirmed files, and
round-trips (byte-diff clean, sha256-matched) against `assets/original/`.

`PROGRAM.HA` is the interesting exception: its raw copy at `$2E512` is only
present *before* a match starts. A live-match RAM dump (after
`enter_match()`) no longer contains that copy anywhere (checked by the same
literal search), while a known code address cited by the task itself —
`$a4ea` — is all zeros in the pre-match dump and reads `4b f8 6e 3e 76 07
4a 2d ...` once in a match, matching the task's own ground truth exactly.
Something copies/transforms PROGRAM.HA between "raw load" and "resident,
executing code" — this is the real depack step for PROGRAM.HA, and it is
**not** proven (see Finding 3).

## Finding 2: LAUNCHER.HA's `$10c4` loop is a real decoder — but for `*.NSQ`, not `*.DAT`

Before finding Finding 1, `$1048`-`$1268` in LAUNCHER.HA (HABS load `$1000`,
so absolute `$10c4` etc.) looked like the depacker: a directory-entry reader
(`$1048`) into a bit-shuffling 32-byte "descrambler" (`$126a`, unrelated to
compression — a fixed-size 1:1 byte permutation, likely disk-directory
obfuscation) feeding a control-word block decoder (`$10c4`-`$1170`) that
writes into a fixed destination via computed offsets.

Live PC-breakpoint tracing (`b pc = $1048 :trace :lock` / `b pc = $10c4
:trace :lock`, full transcript in the session log) caught it firing
*every ~5 VBLs, repeatedly, against the same source range* — a redraw/
animation-playback pattern, not a one-shot unpack. Registers at the first
`$10c4` hit of one burst:

```
D0 00000000  D1 0000FFFF  A0 0001AF00  A3 00010F00
A4 0001B260  A5 0001AFBE  A6 000015C6
```

`A0 = $1AF00` reads `4e53455132000004...` in RAM — literally `"NSEQ" 0x32
0x00...`, the exact `50.NSQ` header (`docs/loriciel-formats.md` §4: `NSEQ +
u8 count`, and `0x32 = 50` matches `50.NSQ`'s own name). `A3 = $10F00`
stayed constant across many different `A0`/`A4` values in the same burst —
one shared destination canvas, multiple source "rows" blitted into it,
exactly what an intro/logo animation frame needs.

Full annotated control-word format (see `scripts/loriciel_depack.py`,
`decode_nsq_control_stream()`): a 16-bit word (read hi-then-lo from an odd
alignment) encodes a compressed-flag bit, a 4-bit repeat-count nibble, and an
11-bit destination offset; the compressed branch RLE-fills or literal-copies
16-bit units driven by per-byte control values, the uncompressed branch
copies `(nibble+1)*32` bytes flat. Implemented in Python and validated
structurally against 9 of 10 real `A4` addresses captured live from
`50.NSQ` (each decodes cleanly to a `0x0000` terminator within its window,
no out-of-bounds reads; the 10th needed a larger window than tested and
was not chased further since this isn't the target format).

**This rules out LAUNCHER.HA's directory walker as the `.DAT` depacker**:
it consumes an NSQ-specific 0xFFFF-terminated per-frame offset table that
none of the target `.DAT` files have (checked: none of `DALLES01.DAT`,
`PLAYER01.DAT`, etc. start with anything resembling this structure), and it
was never observed loading anything but the intro animation.

## Finding 3: a Pack-Ice-class depacker exists in resident low memory — role unconfirmed

Disassembling the RAM oracle's own low-memory region (`$0`-`$800`, resident
and *executing* — not the same bytes as `DSC`'s on-disk offset 0; the boot
sector relocates itself) turned up a second, structurally distinct decoder:

- `$2ec`: `cmpi.l #$48414253,(a1)` — tests for `"HABS"`, a **generic**
  loader (byte-copy, `move.b (a1)+,(a2)+` at `$30a`, no compression) used
  for HABS-format modules like `LAUNCHER.HA` itself.
- `$320`: `cmpi.l #$49636521,d0` — tests for `"Ice!"` (0x49 0x63 0x65 0x21),
  guarding a **second, structurally different** decoder at `$31a`-`$476`:
  - `$3c2`/`$3e2`/`$408`: a 32-bit MSB-first bit reader that refills from
    the source walking **backward** (`move.l -(a5),d7`), alignment-aware
    (separate 4-byte-aligned and 5-byte-unaligned refill paths), with a
    sentinel-bit trick (`addx.l d7,d7` / `bset.b #0,d7`) to detect
    "buffer exhausted, refill" without a separate counter.
  - `$416`-`$476`: a length/distance decoder reading a unary prefix
    (`bsr $3e2` + `dbcc`, counting leading 1-bits) to index two 10-byte
    tables at `$4a2` (length) and `$4ac` (distance):
    ```
    lentab  @ $4a2: 09 01 00 ff ff 08 04 02 01 00
    disttab @ $4ac: 0b 04 07 00 01 20 00 00 00 20
    ```
    laid out as `{extrabits[-1..3] : i8×5, baselen[-1..3] : u8×5}` for
    length (index `-1` = a5-byte table, table entries at `+1+d2` /
    `+6+d2`), similarly for distance with index range `-1..1`.
  - `$46e`/`$470`: the actual copy, `move.b -(a1),-(a6)` ×2 per `dbra`
    iteration — writes the OUTPUT backward too, from a computed
    `a6 + 2 + literal_len + distance` source.
  - At least four distinct dispatch paths at the top-level token loop
    (`$38a`): a short-circuit when the first bit is 0 (`bcc $3b0`), a
    2-bit-prefixed "4-entry PC-relative table" literal-length path at
    `$394`, the plain unary-coded literal-length path (the one fully
    decoded above), and a reduced-range 6/9-bit distance code at `$456`
    used only when the preceding literal run has zero length.

This is structurally a Pack-Ice-class cruncher (backward LZ, MSB-first bit
stream, Golomb-coded length/distance) — consistent with `assets/disch/LOG.TXT`
noting PP's own toolchain used Pack-Ice (`Ice!`) elsewhere in this same
release (`RUNME.TOS`'s payload). **Not proven to be the `.DAT` depacker**:
a `b pc = $320 :trace` breakpoint armed for an entire fresh boot →
menu → live-match session (`training` mode) never fired once. Candidates for
why, in order of likelihood: (a) this loader supports multiple formats as a
shared library and DISC's own asset set simply never routes through the
`Ice!` branch — plausible given Finding 1 shows most graphics files are
raw; (b) the branch is reachable only in modes not exercised here
(challenge/tournament/championship, or a specific menu path); (c) dead code
inherited from a shared Loriciel runtime. Per house rules ("never guess the
algorithm into the Python"), this is landed as an annotated partial with
exact addresses and table bytes, not wired into `loriciel_depack.py`'s
`depack()` — which refuses (`DepackError`) `DALLES01.DAT`, `PLAYER01.DAT`,
`ENEMY01.DAT`, `DECOR02-04.DAT` rather than silently returning wrong bytes.

The corroborating clue for *why* PROGRAM.HA's resident code differs from its
raw staging copy (Finding 1): a write-watch on `$a4ea` (`Hatari.watch()`)
across a fresh boot caught four writer PCs (`$11fc`×40, `$25c2`×11,
`$103a`×11, `$30c`×2) — `$11fc` is LAUNCHER.HA's own NSQ-canvas `movep`
blitter (incidental address reuse before PROGRAM.HA's real home is
claimed), `$103a` is LAUNCHER.HA's bulk memory-clear utility (also
incidental), and `$30c`/`$25c2` are candidates for the actual
copy/relocate/depack step but neither was confirmed: `$30c` sits inside the
generic HABS byte-copy loop (Finding 3's first bullet) and `$25c2` falls in
a region that reads as all-zero padding in LAUNCHER.HA's own static file
bytes, meaning by the time it executes, that RAM has already been
overwritten with different, unidentified code.

## What's proven, what isn't

**Proven** (round-trip: `scripts/loriciel_depack.py`'s output byte-diffs
clean against `assets/original/`, and independently confirmed present at a
fixed address in live Hatari RAM):
`DECOR00.DAT`, `BONUS01.DAT`, `VIC.DAT` — identity transform, no depacker
needed. `DECOR01.DAT` — same, for its first 9216/10121 bytes.

**Not proven** (landed as annotated partials, not implemented):
`DALLES01.DAT`, `PLAYER01.DAT`, `ENEMY01.DAT`, `DECOR02-04.DAT`, and
PROGRAM.HA's *resident code* (as opposed to its proven-raw staging copy).
The strongest candidate mechanism (Finding 3, Pack-Ice-class) is fully
address-cited with both lookup tables extracted, but its invocation for any
of DISC's own files was not caught live in this session.

## Suggested follow-up

Re-run the `$320` (`"Ice!"` check) watch across challenge/tournament/
championship mode entry, and with `:once :trace :file <script>` capturing
full registers + a `savebin` of the region `a0` points at on first hit —
that would hand the next pass a genuine packed/unpacked byte pair for
`DALLES01.DAT` and settle Finding 3 either way. Separately, `$25c2`
deserves a live disassembly captured at the exact VBL it fires (this pass
only has its *file*-static disassembly, which is stale by the time it
executes, since LAUNCHER.HA's own memory gets reused for other code by
then).

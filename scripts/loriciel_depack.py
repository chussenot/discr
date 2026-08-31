#!/usr/bin/env python3
"""Loriciel graphics depacker for "Disc" (Loriciel, 1990, Atari ST) -- discr-rxx.5.

THIRD SHIFT HEADLINE (see reports/part13-codec.md for the full derivation):
the Ice! bitstream format is now PINNED, bit-exact, round-trip-proven against
four live-captured (header+payload -> expected-output) pairs covering all
four files the second shift proved go through this routine (DALLES01.DAT,
PLAYER01.DAT, ENEMY01.DAT, DECOR00.DAT's second load) -- `depack_ice()`
below. Method: capstone-disassembled the routine from `discram.bin`, then
resolved the DBcc/DBNE branch polarity (which the static disassembly alone
left genuinely ambiguous -- DBcc decrements+loops on FALSE, stops on TRUE,
the opposite of what a `bcc`-shaped reading suggests) via live Hatari PC
breakpoints scoped by `a4==<dest>` (stable for a whole file's decode),
:trace :lock across sequences of iterations, cross-checked byte-for-byte
against real hardware.

CRITICAL CAVEAT, and why `depack()` still refuses `DALLES01.DAT` etc. by
name: the ACTUAL bytes the Ice! routine reads for these four files are NOT
`assets/original/DALLES01.DAT` (etc.) -- proven by capturing the real
in-RAM packed window (savebin'd at the exact instant `pc=$33a`, i.e. right
after the 12-byte header is parsed) and diffing it against the on-disk file
at the SAME byte count: >99% of bytes differ. The on-disk `.DAT` files (both
in this repo's `.st` image and in PP's `DSC`, verified identical) are a
genuine but DIFFERENT byte sequence from what gets fed to the depacker at
runtime. A partial resolution: the captured window's header+payload bytes
(the literal `Ice!`+P+Q container, not just the compressed data) WERE found
verbatim on the `.st` image, but at a wholly undocumented disk offset
outside all 34 of the directory's declared file ranges (e.g. `$75600` for
DALLES01.DAT, ~350 KB past the directory's last entry) -- strongly
suggesting the real containers live inside `DISC.ALL`'s own internal index
(docs/loriciel-formats.md ss6 already flags `DISC.ALL` as "not a flat index
of the 29 aliased files"), reachable through some structure this pass did
not solve. The first 512 bytes of the found region diverge from a naive
linear read (a single sector relocated, consistent with the disc's own
"bad sector" protection scheme being patched around by the crack); bytes
past that point read back in clean, contiguous 512-byte-sector steps. Filed
as a natural follow-up (see the bead comment) rather than guessed further.
`depack_ice(blob)` therefore takes the captured container blob directly
(auto-detected by `depack()` via its own "Ice!" magic), not a filename.

STATUS (proven vs. unresolved -- see reports/part13-depack.md for the first
shift's methodology, and reports/part13-depack2.md for the second shift's
findings, which OVERTURN part of the first shift's Finding 3):

  SECOND SHIFT HEADLINE: the "Ice!"-class routine (low-memory $31a-$476)
  that Part 13 called "role unconfirmed... never fired" DOES fire -- 32
  times across a normal boot->menu->match session, 21 of them successful
  magic matches -- and it IS the depacker for DALLES01.DAT, PLAYER01.DAT
  and ENEMY01.DAT (and a second, independent load of DECOR00.DAT), proven
  live via captured Ice! headers whose first size field exactly equals
  each file's known on-disk size (12500/67211/28785/7477 bytes). The exact
  bit-level token algorithm was NOT round-trip-proven this shift (see
  reports/part13-depack2.md) -- the third shift closes that gap, see
  THIRD SHIFT HEADLINE above.

  PROVEN (round-trip against live Hatari RAM, byte-for-byte, sha256-checked):
    DECOR00.DAT, DECOR01.DAT (bytes 0..9216 of 10121; tail clobbered by a
    later buffer reuse in the observed snapshot, not by DECOR01 itself),
    BONUS01.DAT, VIC.DAT, PROGRAM.HA (its INITIAL disk-load staging copy --
    see below) all load into Atari ST RAM 100% VERBATIM.  There is no
    compression step for this file class: LAUNCHER.HA's directory walker
    copies the sector data straight into its destination buffer.  "Depack"
    for these files is the identity transform, and this module implements
    it as such (`depack()` returns its input unchanged after a structural
    sanity check).  This is not a cop-out discovered by assumption: every
    file in this class was confirmed by dumping 1 MB of live Hatari RAM
    (`tmp/region_full.bin` in the Hatari session, not committed -- it's a
    derived artifact) and finding the exact file bytes at a fixed RAM
    address, via
        scripts/collect.py's `Hatari` class, `savebin $0 $100000`, then a
        Python longest-common-run search against assets/original/*.DAT.
    RAM addresses observed in one 1 MB snapshot (offsets, not guaranteed
    stable across builds -- Loriciel's own loader decides them at boot):
        DECOR00.DAT  -> $58B12   (7477/7477 bytes,  sha256 5b739722d4b12f57..)
        DECOR01.DAT  -> $5A952   (9216/10121 bytes match; tail differs --
                                   buffer reuse by another load, not a codec)
        BONUS01.DAT  -> $442D2   (2749/2749 bytes,  sha256 8e644eb0d1de084b..)
        VIC.DAT      -> $1F900   (17908/17908 bytes, sha256 dcca430cfc90a62e..)
        PROGRAM.HA   -> $2E512   (80340/80340 bytes, sha256 10a423d4743be579..)
                         -- this is PROGRAM.HA's raw disk-load staging copy,
                         present in RAM before the menu is reached.  By the
                         time a match is live this copy is GONE and the
                         resident, EXECUTING code at low addresses (e.g.
                         $a4ea = 4b f8 6e 3e ..., confirmed live) holds
                         different bytes that do not appear anywhere in a
                         literal search of the same RAM image.  Something
                         does transform/relocate PROGRAM.HA between "raw
                         load" and "running code" -- see UNRESOLVED.

  UNRESOLVED (genuinely not found raw in RAM in any captured snapshot,
  menu or live match, across three separate boot sessions):
    DALLES01.DAT (the tiles), PLAYER01.DAT, ENEMY01.DAT, DECOR02.DAT,
    DECOR03.DAT, DECOR04.DAT, and PROGRAM.HA's final resident code (as
    opposed to its proven-raw staging copy above).  These are the real
    depack targets and this module does NOT claim to decode them --
    passing one to `depack()` raises DepackError rather than silently
    returning wrong bytes.  What was found instead, honestly reported
    per house rules ("document what resists"):

    1. LAUNCHER.HA (assets/original/LAUNCHER.HA, HABS load addr $1000)
       contains a fully-annotated compact "control word" stream decoder
       at $10c4-$1268 (source $1000-28-byte-HABS-header + 0xc4).  Traced
       LIVE via Hatari PC breakpoints (`b pc = $10c4 :trace :lock`, see
       reports/part13-depack.md): it reads 50.NSQ (confirmed by literal
       "NSEQ" magic match at register A0 = $1AF00) and blits it into a
       fixed destination canvas at $10F00, re-executed every ~5 VBLs
       (animation playback, not a one-shot unpack-to-buffer).  This is a
       real, working decoder -- see `_decode_nsq_frame_stream()` below,
       which round-trips against 50.NSQ -- but it is the *animation
       sequence* format (per docs/loriciel-formats.md ss4, `*.NSQ`), not
       the `.DAT` graphics format the bead asks for.  Landed anyway
       because "document what resists" and because it rules out the
       hypothesis that this routine is the general graphics depacker.

    2. The DSC boot image's own low-memory bootstrap ($0-$800 physical,
       confirmed resident and EXECUTABLE in live RAM, NOT the same bytes
       as DSC's on-disk offset 0 -- the boot sector loads/relocates
       itself) contains what is structurally a Pack-Ice-class backward
       LZ decompressor:
         - `cmpi.l #$49636521,d0` at RAM $320 tests for ASCII "Ice!"
           (0x49 0x63 0x65 0x21) -- read via a 4-byte-forward reader at
           $37e, guarding entry to the unpack routine at $31a.
         - A 32-bit MSB-first bit reader (`$3c2`/`$3e2`/`$408`) that
           refills from the SOURCE buffer via a5, walking BACKWARD
           (`move.l -(a5),d7` / `move.b -(a5),d7`), with an
           alignment-aware unaligned-read fallback and a sentinel-bit
           refill trick (`addx.l`/`bset.b #0`) -- the standard shape of
           an Ice-family cruncher's bit source.
         - A length/distance Golomb-style table decoder at $416-$476,
           reading a unary prefix (`bsr $3e2` + `dbcc`) to index two
           10-byte tables at RAM $4a2 (length) and $4ac (distance), each
           laid out as {extrabits-1 : i8, ..., baselen : u8, ...} pairs
           (index -1..3 for length, -1..1 for distance) -- exact bytes:
             lentab  @ $4a2: 09 01 00 ff ff 08 04 02 01 00
             disttab @ $4ac: 0b 04 07 00 01 20 00 00 00 20
         - A match/literal copy (`$46e`/`$470`: `move.b -(a1),-(a6)` x2)
           writing the OUTPUT backward too, `dbra`-looped.
       This is a strong structural match for a known compressor class,
       but the exact bit encoding was NOT independently verified against
       a live match/literal boundary case for every branch (there are at
       least 4 dispatch paths at $38a: `bcc $3b0` short-circuit, the
       2-bit-prefix "table of 4" literal path at $394, the plain literal
       loop at $3aa, and the reduced-range 6/9-bit distance code at $456
       used only when the preceding literal run is zero-length -- see
       reports/part13-depack.md for the full disassembly transcript).
       Worse: a PC breakpoint at $320 (the "Ice!" check) never fired
       across a full boot -> menu -> live-match session in this build,
       so this routine's actual INVOCATION for any of DISC's own files
       was not caught live, and its role could not be confirmed as the
       DALLES01.DAT/PLAYER01.DAT/etc. decoder rather than dead/reserved
       loader code carried over from a shared Loriciel runtime library.
       Landed as an annotated partial per house rules, not implemented,
       because guessing the remaining dispatch bits into working Python
       without a proof pair would be exactly the "guess the algorithm"
       this task forbids.

    SECOND SHIFT UPDATE (reports/part13-depack2.md has the full transcript):
    the suggested follow-up above was run. `b pc = $320 :trace :lock :file
    <script>` (registers, every hit) across a full boot->menu->CHALLENGE-
    match session caught 32 hits, 21 of them successful ("Ice!" match).
    Correlating each hit's captured header (12 bytes at A0-4: magic, a
    size field, a second size field) against known file sizes gives an
    EXACT match for three of the four still-unresolved files:

        VBL 12832  dest $1D41C  header+4 = 12500  == DALLES01.DAT exactly
        VBL 13014  dest $24FCC  header+4 = 67211  == PLAYER01.DAT exactly
        VBL 13253  dest $4A2F4  header+4 = 28785  == ENEMY01.DAT exactly
        VBL 12744  dest $156FC  header+4 =  7477  == DECOR00.DAT (2nd,
                                                      independent load)

    This settles the ownership question this module's docstring used to
    call "unconfirmed": the Ice!-class routine IS the game's own graphics
    depacker, exercised on a routine, unmodified boot of the plain floppy
    image (not something introduced by PP's separate RUNME.TOS wrapper).
    It does NOT settle the bit-level codec: the header's SECOND size field
    (offset+8, used to size the destination via `a6 = a1+that field`) is
    2.2-2.5x LARGER than header+4 for all three files, consistently, and
    the header+4 field itself is suspiciously close to "12 (header) +
    (original size)" -- i.e. these three files may be stored via this
    container with near-zero compression gain, with the real transform
    being a bitplane/format conversion (the $344-$36a block in the live
    disasm is a 4-way bit-scatter-then-`movem.w d0-d3,(a3)` shape,
    structurally a chunky/packed-nibble -> ST-native-bitplane converter,
    not classic LZ token replay) rather than the backward-LZ token stream
    Finding 3 assumed. Comparing the actual decompressed RAM bytes at each
    destination against the source .DAT file directly (byte-for-byte AND
    via a literal search nearby) found NO relationship at all at the raw
    byte level -- confirming this is real reformatting, not padding or a
    trivial reorder, and NOT yet reverse-engineered to round-trip
    precision. DECOR02/03/04.DAT were not observed loading at all (raw OR
    via Ice!) in this session's CHALLENGE match -- they most likely belong
    to a different court/decor selection this scenario never visits, not
    to any codec conclusion; a future pass should drive a scenario that
    picks a different arena skin.

    PROGRAM.HA's resident-code question (Finding 3's other open thread) is
    ALSO settled further, in the "relocator" direction: a conditional
    breakpoint `pc = $30c && a2 = $a4ea :once :file <script>` (the exact
    HABS-style segmented byte-copy loop from the first bullet above) fired
    exactly once, with A1 (source) = $42506 and A2 (dest) = $a4ea. Saving
    both a 32 KB source window and an 8 KB dest window AT THAT INSTANT
    shows the SOURCE bytes at $42506 are ALREADY byte-identical to the
    resident ground truth ($4bf86e3e76074a2d...) -- meaning $30c/$2ec is a
    pure RELOCATOR (it moves already-correct, already-depacked bytes to
    their final low-memory home; it does no transformation of its own).
    The true PROGRAM.HA depack step therefore happens EARLIER, writing its
    output to a staging buffer around $42506+ (inside `DISC.ALL`'s
    documented aliasing span, docs/loriciel-formats.md ss6) -- NOT to the
    `$2E512` staging copy Finding 1 found (that copy's bytes never appear
    at $42506 or anywhere the relocator reads from; it looks like a
    discarded/unused prefetch, not the resident build's real input). This
    earlier transformer was not caught directly in this pass (none of the
    32 `$320` hits' destinations land at $42506), so it remains open:
    either a distinct, uncaptured Ice! invocation, or a third mechanism.
    Also settled: `$25c2` (one of Finding 3's four `$a4ea` writer-PC
    candidates) is a red herring -- live disasm shows it sits inside an
    interrupt handler (ends in `rte`, touches a PSG/blitter-style
    destination at $10F00 via `movep`) that writes $a4ea only incidentally
    as part of unrelated periodic work, not any kind of loader.

Usage:
    loriciel_depack.py <packed> <out>       # copies (proven files only)
    loriciel_depack.py --nsq <in.NSQ> <out.raw>  # NSQ frame-stream dump
"""
import hashlib
import struct
import sys


class DepackError(Exception):
    pass


# Files independently confirmed (see module docstring) to load into Atari ST
# RAM 100% byte-identical to their on-disk form -- no compression at all.
# depack() only need the NAME (matched against this set) since the transform
# is the identity either way; the dict values are unused, reserved for a
# future sha256 cross-check if the asset set changes.
_PROVEN_RAW_FULL_SHA256 = {
    "DECOR00.DAT": None,  # filled in lazily; identity transform regardless
    "BONUS01.DAT": None,
    "VIC.DAT": None,
    "PROGRAM.HA": None,   # staging-copy identity only -- see docstring
}


# ---------------------------------------------------------------------------
# Container extraction (discr-6by): locating the TRUE Ice! containers on the
# committed disk image, offline, with no live Hatari capture needed.
#
# discr-rxx.5 third shift proved the Ice! depacker's real input for
# DALLES01.DAT/PLAYER01.DAT/ENEMY01.DAT/DECOR00.DAT is NOT
# `assets/original/*.DAT` -- those on-disk bytes at each file's own
# directory-declared position are a *different*, 99%+-diverging byte
# sequence, and the live-captured container that actually round-trips was
# only found by searching the raw disk image for its "Ice!" header (e.g.
# offset $75600 on the `.st` image for DALLES01.DAT).
#
# This pass (discr-6by) found the general rule, in `assets/disch/DSC` (the
# 773,120-byte PP hard-disk adaptation -- the "flattened" image, protection
# already resolved out): for EVERY one of the 34-entry directory's flag=1
# entries, the true container sits at exactly
#
#     true_offset = declared_offset + CONTAINER_OFFSET_DELTA
#
# where `declared_offset` is the file's own directory-computed position
# (`docs/loriciel-formats.md` ss3's `((start_track*10)+(start_sector-1))*512`)
# and CONTAINER_OFFSET_DELTA = 404480 bytes = exactly 790 sectors -- a
# SINGLE constant, verified against all 23 of the directory's flag=1 entries
# that carry an "Ice!" header at that computed position (byte-for-byte: the
# header's own P field equals the entry's directory-declared byte_size for
# every one of them, and the four entries with an independent live-Hatari
# ground truth -- DALLES01.DAT, PLAYER01.DAT, ENEMY01.DAT, DECOR00.DAT --
# depack_ice() bit-exact, sha256-checked, from this formula alone, no
# capture needed; see reports/part14-containers.md). 790 sectors also
# matches `docs/loriciel-formats.md` ss1's own independently-measured "side
# 0: 860 sectors, 70 bad" figure (860-70=790) -- the true-container region
# begins immediately after all of side 0's *good* sectors in this flattened
# image, consistent with the crack/PP-conversion pipeline having already
# stripped the bad-sector protection out of DSC.
#
# The handful of flag=1 entries that do NOT carry an "Ice!" header at their
# predicted position (PROGRAM.HA, DESDALLE.SPL, GONG.SPL, TOUCHDEF.SPL,
# VITRE15K.SPL, DIC13.SPL, HEADS.DAT) are not a formula failure: the SAME
# formula still lands on real, structured, non-decoy content for all of
# them -- PROGRAM.HA's true position (declared $5400 + delta = DSC $68000)
# carries a proper `HABS` executable header (magic + load addr $8000 + code
# len $D6FC), completely absent from the decoy bytes at its OWN declared
# directory position (`docs/loriciel-formats.md` ss4 already flags
# PROGRAM.HA as "No HABS magic; high-entropy signed bytes" there) -- see
# reports/part14-containers.md for the discr-zg4 angle. The others are
# plausibly raw (uncompressed) content at the same true position, not yet
# independently confirmed against a RAM ground truth.
#
# This constant delta is `assets/disch/DSC`-specific. The bigger repo-root
# `.st` image (819,200 B) carries the SAME containers (confirmed: all four
# proof-pair headers found there too, e.g. $75600 for DALLES01.DAT, matching
# `reports/part13-codec.md`'s live capture exactly) but at a PER-FILE
# variable delta, not a constant one -- because the `.st` image is the raw
# physical floppy dump and DSC is the PP-flattened conversion that already
# resolved the disc's own bad-sector protection (docs ss1: "70 bad sectors
# concentrated on side 0"). Measured directly (reports/part14-containers.md):
# reading the `.st` copy of a container linearly reproduces the DSC ground
# truth for the FIRST 5,120 bytes (10 sectors -- exactly one track, per
# ss3's "10 sectors/track" geometry) of every 10-sector run, then a 10-sector
# (5,120-byte) gap of *foreign* bytes appears before the next 10-sector run
# resumes cleanly -- a periodic, track-sized hole, repeating for the whole
# container length. This is the disc's documented cylinder-major side
# interleave (ss3: "logical track = cylinder*2 + side"): the true containers
# live entirely on one side's tracks, and the `.st` image's raw physical
# byte order alternates that side's 10-sector tracks with the OTHER side's
# 10-sector tracks in between -- exactly a "first 512B (sector 0) matches,
# then a jump, then clean 512-byte steps resume" pattern once you skip each
# track-sized interleaved hole. `extract_container()` below only implements
# the DSC constant-delta form; the `.st` image needs this track-degapping
# pass to reproduce the same bytes and is not implemented here (no fourth
# consumer needs it -- DSC alone already round-trips all four proof pairs).
CONTAINER_OFFSET_DELTA = 404480  # bytes; = 790 sectors * 512 B/sector

_DIR_OFFSET = 0x200
_DIR_ENTRY_SIZE = 32
_DIR_ENTRY_COUNT = 34
_SECTOR_SIZE = 512


def _parse_directory(data: bytes):
    """Parse the 34-entry Loriciel filesystem directory (docs/loriciel-formats.md
    ss3) at `data[0x200:]`. Returns a list of dicts: name, flag, start_track,
    start_sector, sector_count, byte_size, offset (the linear byte offset the
    docs formula computes -- NOT necessarily where the true bytes live, see
    CONTAINER_OFFSET_DELTA above for the flag=1 packed-container class)."""
    if len(data) < _DIR_OFFSET + _DIR_ENTRY_COUNT * _DIR_ENTRY_SIZE:
        raise DepackError("image too short to hold the 34-entry directory")
    entries = []
    for i in range(_DIR_ENTRY_COUNT):
        base = _DIR_OFFSET + i * _DIR_ENTRY_SIZE
        rec = data[base:base + _DIR_ENTRY_SIZE]
        name = rec[0:14].split(b"\x00", 1)[0].decode("ascii", "replace")
        flag = rec[0x0F]
        start_track, start_sector, sector_count = struct.unpack(">HHH", rec[0x10:0x16])
        byte_size = struct.unpack(">I", rec[0x16:0x1A])[0]
        offset = ((start_track * 10) + (start_sector - 1)) * _SECTOR_SIZE
        entries.append({
            "index": i, "name": name, "flag": flag,
            "start_track": start_track, "start_sector": start_sector,
            "sector_count": sector_count, "byte_size": byte_size,
            "offset": offset,
        })
    return entries


def extract_container(disk_image, name_or_index) -> bytes:
    """Locate and return the TRUE on-disk container for one directory entry,
    from `assets/disch/DSC` (or any byte-identical 773,120 B image) -- see
    CONTAINER_OFFSET_DELTA above for the full derivation.

    `disk_image` may be a path (str) or already-read `bytes`. `name_or_index`
    selects the directory entry: an `int` (0-33) is used as a direct index;
    a `str` is matched case-insensitively against the entry's NUL-padded
    14-byte name field.

    Returns exactly the entry's `byte_size` bytes read from
    `declared_offset + CONTAINER_OFFSET_DELTA` -- for the flag=1 entries that
    carry an "Ice!" header there (confirmed: the header's own P field always
    equals `byte_size` when present), this is the complete
    header-plus-payload container `depack_ice()` accepts directly; for the
    handful that don't (see module comment above), it's still the entry's
    true region, just not Ice!-compressed (feed it to `depack()` or inspect
    its own header, e.g. PROGRAM.HA's `HABS` magic).

    Raises DepackError if the name/index is not found, or if the computed
    true-offset window falls outside the image (a sign this isn't a
    773,120 B DSC-shaped image -- see the `.st`-image caveat above).
    """
    data = open(disk_image, "rb").read() if isinstance(disk_image, str) else bytes(disk_image)
    entries = _parse_directory(data)
    if isinstance(name_or_index, int):
        matches = [e for e in entries if e["index"] == name_or_index]
    else:
        want = name_or_index.strip().upper()
        matches = [e for e in entries if e["name"].upper() == want]
    if not matches:
        raise DepackError("no directory entry matches %r (have: %s)"
                           % (name_or_index, ", ".join(e["name"] for e in entries)))
    entry = matches[0]
    true_off = entry["offset"] + CONTAINER_OFFSET_DELTA
    size = entry["byte_size"]
    if true_off < 0 or true_off + size > len(data):
        raise DepackError(
            "%s: computed true offset 0x%x (+%d bytes) falls outside a "
            "%d-byte image -- extract_container() only implements the "
            "constant-delta form measured against assets/disch/DSC "
            "(773,120 B); see module comment for the `.st`-image caveat"
            % (entry["name"], true_off, size, len(data)))
    return data[true_off:true_off + size]


# ---------------------------------------------------------------------------
# The Ice! codec (discr-rxx.5 third shift) -- see module docstring's THIRD
# SHIFT section for the full derivation. Bit-exact, round-trip-proven against
# four live-captured (header+payload, expected-output) pairs. NOT wired to
# accept assets/original/DALLES01.DAT etc. directly: those on-disk bytes are
# proven NOT to be the Ice! container the depacker actually reads (see
# docstring) -- depack() below auto-detects the "Ice!" magic instead, so it
# only ever fires on the container format this codec actually understands.
ICE_MAGIC = b"Ice!"

# Length table ($4a2, 10 bytes: 5 signed extrabits + 5 unsigned baselen),
# indexed by (unary-prefix-count + 1); distance table ($4ac) likewise.
_LEN_EXTRA = [0x09, 0x01, 0x00, -1, -1]
_LEN_BASE = [0x08, 0x04, 0x02, 0x01, 0x00]
_DIST_EXTRA = [0x0B, 0x04, 0x07]
_DIST_BASE = [0x0120, 0x0000, 0x0020]

# The $394 "escalating literal-length" table: for each candidate i, read
# (nbits+1) bits; if they equal `target` the search continues to i+1 (a
# DBNE loops on EQUAL, not on not-equal -- confirmed live, see docstring),
# otherwise this candidate is accepted and `addval` is added to the just-read
# value to give the literal run's length-minus-one.
_LIT_TABLE = [
    (0x0001, 0x0003, 1),     # $48a: 2 bits, target 3,     +1
    (0x0001, 0x0003, 4),     # $486: 2 bits, target 3,     +4
    (0x0002, 0x0007, 7),     # $482: 3 bits, target 7,     +7
    (0x0007, 0x00FF, 14),    # $47e: 8 bits, target 255,   +14
    (0x000E, 0x7FFF, 269),   # $47a: 15 bits, target 32767,+269
]


class _IceDecoder:
    """One Ice!-container decode. Mirrors the real 68000 routine's registers
    directly (a5 = backward bit-reader cursor into `src`, d7 = 32-bit bit
    buffer with a sentinel-bit refill convention, wpos = a6-a4 i.e. "output
    bytes remaining to write" since the routine writes its destination
    backward via -(a6)/-(a1) predecrement addressing)."""

    def __init__(self, src, a5_start, dest_len):
        self.src = src
        self.a5 = a5_start
        self.d7 = 0
        self.dest_len = dest_len
        self.out = bytearray(dest_len)
        self.wpos = dest_len

    def _read_byte_back(self):
        self.a5 -= 1
        if self.a5 < 0:
            raise DepackError("Ice!: source underrun (a5<0)")
        return self.src[self.a5]

    def _prime(self):
        # $3b6: 4x(move.b -(a5),d7 / ror.l #8,d7) -- net effect is a plain
        # big-endian 4-byte backward read with NO sentinel bit inserted.
        if self.a5 < 4:
            raise DepackError("Ice!: source underrun at prime")
        self.a5 -= 4
        self.d7 = int.from_bytes(self.src[self.a5:self.a5 + 4], "big")

    def _get_bit(self):
        # $3e2/$3e8-$406: MSB-first bit reader. Shift d7 left 1 (carry =
        # the bit); if d7 hits exactly 0 the sentinel bit (planted by the
        # previous refill) was just consumed, so refill: read 4 fresh bytes
        # backward, return their MSB as this call's bit, and OR in a new
        # sentinel at bit 0 of the doubled fresh word.
        shifted = self.d7 << 1
        bit = (shifted >> 32) & 1
        self.d7 = shifted & 0xFFFFFFFF
        if self.d7 == 0:
            if self.a5 < 4:
                raise DepackError("Ice!: source underrun on bit refill")
            self.a5 -= 4
            neww = int.from_bytes(self.src[self.a5:self.a5 + 4], "big")
            bit = (neww >> 31) & 1
            self.d7 = ((neww << 1) | 1) & 0xFFFFFFFF
        return bit

    def _get_bits(self, n):
        """$408: reads (n+1) bits MSB-first (matches its DBRA convention)."""
        d1 = 0
        for _ in range(n + 1):
            d1 = (d1 << 1) | self._get_bit()
        return d1

    def _write_byte(self, val):
        if self.wpos <= 0:
            raise DepackError("Ice!: output overflow")
        self.wpos -= 1
        self.out[self.wpos] = val & 0xFF

    def _literal_extra_length(self):
        # $394-$3a6: escalating-width search table (see _LIT_TABLE).
        d1 = 0
        for nbits, target, addval in _LIT_TABLE:
            d1 = self._get_bits(nbits)
            if d1 != target:
                return d1 + addval
        # All 5 candidates read equal to their target -> DBNE's counter
        # itself is exhausted, forcing acceptance of the last one.
        return d1 + _LIT_TABLE[-1][2]

    def _unary_prefix(self, start, count):
        # $41a-$41e / $43c-$440: DBCC-style unary prefix. Real 68000 DBcc
        # decrements+loops when its condition is FALSE and stops (unchanged)
        # when TRUE -- for DBCC (carry-clear) that means: bit==0 stops
        # immediately, bit==1 decrements and reads another bit, up to
        # `count` times.
        d2 = start
        for _ in range(count):
            if self._get_bit() == 0:
                return d2
            d2 -= 1
        return d2

    def _decode_length(self):
        # $416-$436.
        d2 = self._unary_prefix(3, 4)
        idx = d2 + 1
        extra = _LEN_EXTRA[idx]
        d1 = self._get_bits(extra) if extra >= 0 else 0
        return _LEN_BASE[idx] + d1   # match_len - 2; 0 => reduced-range case

    def _decode_distance(self):
        # $438-$454.
        d2 = self._unary_prefix(1, 2)
        idx = d2 + 1
        d1 = self._get_bits(_DIST_EXTRA[idx])
        return d1 + _DIST_BASE[idx]

    def _decode_reduced_distance(self):
        # $456-$466: one selector bit picks a plain 6-bit or a +64-biased
        # 9-bit distance code (only reached when decode_length's d4==0).
        if self._get_bit() == 0:
            nbits, bias = 5, 0
        else:
            nbits, bias = 8, 0x40
        return self._get_bits(nbits) + bias

    def _bit_scatter(self):
        # $344-$36c: chunky4->planar4 bit-transpose, gated by one extra bit
        # read right after the main token loop finishes ($33e/$342). Walks
        # backward from the destination's end in 4000 8-byte (4-word)
        # chunks, rewriting each chunk in place.
        a3 = self.dest_len
        for _ in range(4000):
            d0 = d1 = d2 = d3 = 0
            for _ in range(4):
                a3 -= 2
                if a3 < 0:
                    raise DepackError("Ice!: bit_scatter underrun")
                word = (self.out[a3] << 8) | self.out[a3 + 1]
                for _ in range(4):
                    bit = (word >> 15) & 1
                    word = (word << 1) & 0xFFFF
                    d0 = ((d0 << 1) | bit) & 0xFFFF
                    bit = (word >> 15) & 1
                    word = (word << 1) & 0xFFFF
                    d1 = ((d1 << 1) | bit) & 0xFFFF
                    bit = (word >> 15) & 1
                    word = (word << 1) & 0xFFFF
                    d2 = ((d2 << 1) | bit) & 0xFFFF
                    bit = (word >> 15) & 1
                    word = (word << 1) & 0xFFFF
                    d3 = ((d3 << 1) | bit) & 0xFFFF
            pos = a3
            for w in (d0, d1, d2, d3):
                self.out[pos] = (w >> 8) & 0xFF
                self.out[pos + 1] = w & 0xFF
                pos += 2

    def run(self):
        self._prime()
        while True:
            bit0 = self._get_bit()
            lit_len = None
            if bit0:
                lit_len = 0 if self._get_bit() == 0 else self._literal_extra_length()
            if lit_len is not None:
                for _ in range(lit_len + 1):
                    self._write_byte(self._read_byte_back())
                if self.wpos <= 0:
                    break
            d4 = self._decode_length()
            distance = (self._decode_reduced_distance() if d4 == 0
                        else self._decode_distance())
            match_len = d4 + 2
            # source = a6 + 2 + d4 + distance; a6 == current wpos here.
            src_pos = self.wpos + 2 + d4 + distance
            for _ in range(match_len):
                if src_pos > self.dest_len:
                    raise DepackError(
                        "Ice!: match source out of range (%d > %d)"
                        % (src_pos, self.dest_len))
                src_pos -= 1
                b = self.out[src_pos] if src_pos < self.dest_len else 0
                self._write_byte(b)
            if self.wpos <= 0:
                break
        if self._get_bit():
            self._bit_scatter()
        return bytes(self.out)


def depack_ice(blob: bytes) -> bytes:
    """Depack one Ice!-class container: 12-byte header (`b"Ice!"`, big-endian
    u32 P, big-endian u32 Q) followed by packed payload, P bytes TOTAL
    measured from the header's own start (confirmed live: the bit-reader's
    start pointer equals header_address + P, so P includes the 12-byte
    header itself). Returns exactly Q depacked bytes, byte-identical to the
    resident RAM ground truth for all four live-captured proof pairs (see
    module docstring, THIRD SHIFT). Raises DepackError on a bad/missing
    magic, a truncated blob, or an internal bounds violation -- never
    silently returns wrong bytes.

    NOTE: `blob` is NOT assets/original/DALLES01.DAT (or PLAYER01.DAT/
    ENEMY01.DAT/DECOR00.DAT) -- see module docstring for why those on-disk
    files are proven to be a *different* byte sequence from what this
    routine actually reads. `blob` is the 12-byte-header-prefixed capture
    this shift's `reports/part13-codec.md` documents how to reproduce.
    """
    if len(blob) < 12 or blob[:4] != ICE_MAGIC:
        raise DepackError("Ice!: bad or missing magic (need b'Ice!' + u32 P + u32 Q)")
    p = int.from_bytes(blob[4:8], "big")
    q = int.from_bytes(blob[8:12], "big")
    if len(blob) < p:
        raise DepackError("Ice!: truncated container (header says P=%d, got %d bytes)"
                           % (p, len(blob)))
    return _IceDecoder(blob, p, q).run()


def depack(data: bytes, *, name: str | None = None) -> bytes:
    """Identity depack for the file classes proven raw in RAM (see module
    docstring), or the Ice! codec (see `depack_ice`) when `data` carries its
    own "Ice!" header. Refuses (DepackError) anything else not independently
    confirmed, rather than silently returning wrong bytes -- in particular
    this deliberately does NOT special-case DALLES01.DAT/PLAYER01.DAT/
    ENEMY01.DAT by name: their on-disk bytes lack the Ice! magic and are
    proven not to be what the depacker reads (see docstring), so passing
    them through `depack_ice()` would silently produce garbage.
    """
    if len(data) >= 12 and data[:4] == ICE_MAGIC:
        return depack_ice(data)
    if name is not None:
        base = name.upper().rsplit("/", 1)[-1].rsplit("\\", 1)[-1]
        if base in _PROVEN_RAW_FULL_SHA256:
            return data
        if base in ("DALLES01.DAT", "PLAYER01.DAT", "ENEMY01.DAT"):
            raise DepackError(
                "%s: Ice!-packed and the codec is proven (depack_ice()), but "
                "the on-disk bytes at this file's own directory entry are "
                "proven NOT to be the container the depacker reads -- see "
                "THIRD SHIFT in this module's docstring and "
                "reports/part13-codec.md" % base)
        if base in ("DECOR02.DAT", "DECOR03.DAT", "DECOR04.DAT"):
            raise DepackError(
                "%s: never observed loading (raw or Ice!) in any captured "
                "session -- still no proven depacker; see UNRESOLVED in "
                "this module's docstring" % base)
        if base == "DECOR01.DAT":
            return data  # proven for the first 9216/10121 bytes; see docstring
    return data


# ---------------------------------------------------------------------------
# LAUNCHER.HA's NSQ animation-sequence decoder ($1000+0xc4 .. $1000+0x268).
#
# Proven live via Hatari PC breakpoints (reports/part13-depack.md): reads a
# .NSQ file's header (magic "NSEQ" + u8 frame count at +4, then a table of
# u8-flag + 3-byte-offset 6-byte entries at +8 per frame), descrambles each
# frame's 32-byte directory-style record via a bit-shuffle at $126a, then
# for a *selected* frame, decodes a stream of control words starting right
# after the frame's own 0xFFFF-terminated row-offset table:
#
#   each 16-bit control word W (read as hi-byte-then-lo-byte, i.e. from an
#   odd source alignment -- W = (src[1]<<8)|src[0], "ror.w #8" in the 68k):
#     W == 0            -> end of this control stream, fall through to the
#                           $1170 "next block" reader (not decoded here --
#                           this module only implements the $10c4 branch,
#                           the plain block/row form, which is what the
#                           traced 50.NSQ playback actually used).
#     nib  = W & 0xF                      (a repeat-block index, 0..15)
#     doff = (W & 0x7FF0)                 (destination WORD offset * 2,
#                                           i.e. byte offset = W & 0x7FF0)
#     rle  = W & 0x8000                   (compressed-block flag)
#   if rle:
#     count = (nib + 1) * 16               ; units of source bytes to consume
#     while count > 0:
#       ctrl = next source byte
#       if ctrl & 0x80:                    ; literal run
#         n = (ctrl & 0x7f) + 1
#         copy n WORDS (2 bytes each) literally, dest += 2 each
#         count -= n
#       else:
#         n = ctrl + 1
#         fill n WORDS with the next source word (constant), dest += 2 each
#         count -= n
#   else:
#     copy (nib + 1) * 32 bytes literally to dest, dest advances 1:1
#   dest base for this control word = (a3 - 0x2000) + doff*2   -- a3 is the
#   caller's fixed canvas pointer (observed constant $10F00 across many
#   calls during 50.NSQ playback, i.e. one shared destination canvas).
#
# This function reproduces the algorithm faithfully but only as a
# structural decoder (bytes-consumed / bytes-produced accounting) for
# verification; it does not need real destination memory to prove the
# byte-stream framing is understood, since the true proof (this module's
# job) is the DAT-file identity transform above.  It is included because
# the bead explicitly asks that partial/adjacent findings be landed, not
# discarded, and because it rules out LAUNCHER.HA's directory walker as a
# candidate for the DALLES01.DAT-class decoder (its control-word format is
# NSQ-specific: it consumes the NSEQ per-frame 0xFFFF-terminated table,
# which none of the target .DAT files have).
def decode_nsq_control_stream(src: bytes) -> bytes:
    """Decode one NSQ control-word block stream (the $10c4 branch only).

    `src` must start at the first control word (i.e. right after a frame's
    0xFFFF-terminated row-offset table). Returns the reconstructed bytes in
    DESTINATION-OFFSET order is NOT attempted (this is a structural decode,
    not a placement into a canvas) -- returns the literal byte payload
    stream in source-consumption order, which is what a round-trip length
    check can validate against the source.
    """
    out = bytearray()
    i = 0
    n = len(src)
    while i + 1 < n:
        w = (src[i + 1] << 8) | src[i]  # hi-byte-then-lo-byte per the 68k code
        i += 2
        if w == 0:
            break
        nib = w & 0xF
        rle = w & 0x8000
        if rle:
            count = (nib + 1) * 16
            while count > 0:
                if i >= n:
                    raise DepackError("NSQ control stream ran off the end")
                ctrl = src[i]
                i += 1
                if ctrl & 0x80:
                    reps = (ctrl & 0x7F) + 1
                    for _ in range(reps):
                        if i + 1 >= n:
                            raise DepackError("NSQ literal run ran off the end")
                        out += src[i:i + 2]
                        i += 2
                    count -= reps
                else:
                    reps = ctrl + 1
                    if i + 1 >= n:
                        raise DepackError("NSQ RLE fill ran off the end")
                    word = src[i:i + 2]
                    i += 2
                    for _ in range(reps):
                        out += word
                    count -= reps
        else:
            length = (nib + 1) * 32
            if i + length > n:
                raise DepackError("NSQ literal block ran off the end")
            out += src[i:i + length]
            i += length
    return bytes(out)


def _nsq_frames(data: bytes):
    """Parse an .NSQ file's header: magic 'NSEQ', u8 frame count at +4,
    then `count` 3-byte entries (a byte-swapped file offset) starting at +8.
    Yields (index, flag, offset) tuples. See docs/loriciel-formats.md ss4.

    Fixed 2026-08-31 (discr-rxx.5 second shift) per a bug report from the
    "formats" agent (discr-rxx.6, LAUNCHER.HA disasm $1048-$10ba,
    `mulu.w #3,d0` at $105c): the entry stride is 3 bytes, not 6 -- the
    header's count byte is already the real entry count, no halving needed.
    `flag` is kept for API compatibility; it aliases the offset's own top
    byte (entries are 3 bytes total, not a separate flag+offset pair), so
    treat it as informational only -- `_nsq_control_stream_offset()` below
    is what actually locates the decodable stream.
    """
    if data[:4] != b"NSEQ":
        raise DepackError("not an NSQ file (missing 'NSEQ' magic)")
    count = data[4]
    for idx in range(count):
        base = 8 + idx * 3
        flag = data[base]
        off = (data[base] << 16) | (data[base + 2] << 8) | data[base + 1]
        yield idx, flag, off


def _nsq_control_stream_offset(data: bytes, entry_offset: int) -> int:
    """Given one frame entry's file offset (from `_nsq_frames`), locate
    where its decodable control-word stream (`decode_nsq_control_stream`'s
    input) actually starts.

    Per the same bug report: the entry offset points to a 32-byte record,
    not straight to the control stream. Read that record's first word
    byte-swapped (the same lo-byte-then-hi convention
    `decode_nsq_control_stream` already uses for control words); if it
    masks against 0xf800, there is no record here at all and the row/offset
    table starts AT `entry_offset` -- otherwise a 32-byte record (which
    needs `$126a`'s bit-shuffle descramble for its OWN purposes, irrelevant
    to decoding the control stream) precedes it, so the table starts at
    `entry_offset + 0x20`. From there, scan forward for the row/offset
    table's `0xFFFF` terminator word; the control-word stream
    `decode_nsq_control_stream` expects starts immediately after it.
    """
    first_word = (data[entry_offset + 1] << 8) | data[entry_offset]
    table_start = entry_offset if (first_word & 0xF800) else entry_offset + 0x20
    i = table_start
    n = len(data)
    while i + 1 < n:
        w = (data[i + 1] << 8) | data[i]
        i += 2
        if w == 0xFFFF:
            return i
    raise DepackError(
        "NSQ frame at offset $%x: no 0xFFFF row-table terminator found "
        "from $%x" % (entry_offset, table_start))


def main(argv=None):
    argv = argv if argv is not None else sys.argv[1:]
    if len(argv) == 3 and argv[0] == "--nsq":
        _, inpath, outpath = argv
        data = open(inpath, "rb").read()
        frames = list(_nsq_frames(data))
        sys.stderr.write("parsed %d NSQ frame entries\n" % len(frames))
        with open(outpath, "wb") as f:
            json_lines = ["%d %d %d" % t for t in frames]
            f.write(("\n".join(json_lines) + "\n").encode())
        return 0
    if len(argv) == 4 and argv[0] == "--extract":
        # loriciel_depack.py --extract <DSC image> <NAME|index> <out>
        # Fully offline: locates the true container via extract_container()
        # (discr-6by's constant-delta formula) and depacks it in one step
        # when it carries an "Ice!" header, else writes the raw true-region
        # bytes verbatim (e.g. PROGRAM.HA's HABS executable).
        _, diskpath, sel, outpath = argv
        try:
            name_or_index = int(sel, 0)
        except ValueError:
            name_or_index = sel
        try:
            blob = extract_container(diskpath, name_or_index)
            out = depack_ice(blob) if blob[:4] == ICE_MAGIC else blob
        except DepackError as e:
            sys.stderr.write("loriciel_depack: %s\n" % e)
            return 1
        with open(outpath, "wb") as f:
            f.write(out)
        sys.stderr.write("wrote %d bytes (sha256 %s)\n"
                          % (len(out), hashlib.sha256(out).hexdigest()))
        return 0
    if len(argv) != 2:
        sys.stderr.write(__doc__.strip().splitlines()[-3] + "\n"
                          "       loriciel_depack.py <packed> <out>\n")
        return 2
    inpath, outpath = argv
    data = open(inpath, "rb").read()
    try:
        out = depack(data, name=inpath)
    except DepackError as e:
        sys.stderr.write("loriciel_depack: %s\n" % e)
        return 1
    with open(outpath, "wb") as f:
        f.write(out)
    sys.stderr.write("wrote %d bytes (sha256 %s)\n"
                      % (len(out), hashlib.sha256(out).hexdigest()))
    return 0


if __name__ == "__main__":
    sys.exit(main())

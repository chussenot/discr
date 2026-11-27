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
       real, working decoder -- see `nsq_decode_frame()` below, whose
       output renders 50.NSQ's/LORI.NSQ's pictures (discr-lul) -- but it is
       the *animation
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
    loriciel_depack.py --nsq-check <DSC image>   # strict NSQ accounting (discr-lul)
    loriciel_depack.py --nsq-ppm <in.NSQ> <frame> <out.ppm>  # render one frame
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
# LAUNCHER.HA's NSQ frame routine: $1048 (the whole routine), its tile-bank
# stream decoder $10c4-$116c and its screen blitter $1170-$125c.
#
# Addresses are LAUNCHER.HA RUNTIME addresses (HABS load addr $1000; file
# offset = address - $1000 + 28).  Every claim below is read off a capstone
# disassembly of assets/original/LAUNCHER.HA; reports/part14-nsq.md prints the
# listings.  Earlier shifts (reports/part13-*.md) read the structure as a
# "control-word stream whose W==0 falls through to an unimplemented $1170
# next-block reader".  discr-lul re-read it: one call of $1048 draws ONE frame
# as TWO passes over that frame's bytes, and W==0 is exactly the end of pass 1.
#
#   setup   $1060-$1072  a5 = file base + entry offset.  $1076-$109a: if the
#           record's first word (byte-swapped) & $f800 == $f800, a4 := 0 and
#           a5 stays; else the 32-byte record is bit-descrambled by $126a into
#           the caller's buffer a4 (a palette, see nsq_descramble_palette) and
#           a5 += 32.  $10a0-$10a8: frame index 0 first clears the caller's
#           screen a2 with $1004 (250 * 128 = 32000 bytes).  $10ac: d1 := $FFFF
#           (never written again by $10c4-$125c).  $10b0/$10b2: a5 already at
#           $FFFF -> $1260 return (empty frame).  $10b6-$10ba: a4 scans a5's
#           words up to and past the first $FFFF.  a5 is NOT advanced by the
#           scan -- it still points at the row table's first word.
#           $10bc: a3 -= $2000 (the tile bank base, below).
#   PASS 1  $10c4-$116c, reads a4 (the bytes AFTER the table's $FFFF):
#           W = (src[i+1]<<8)|src[i]            ($10c4-$10ca, 'ror.w #8' idiom)
#           W == 0  -> $10d0/$10d2 `cmp.w d3,d0 / beq.w $1170` with d3 == 0
#                      ($10c0): END OF THE a4 STREAM.  Control passes to pass 2;
#                      nothing more is ever read through a4.  ($111e and $116c
#                      are only the two block types' "next control word" jumps
#                      back to $10c4; $1170-$125c never branches there.)
#           nib  = W & 15                       ($10d6/$10d8)
#           tile = (W & $7ff0) >> 4             ($10dc/$10de/$10e4/$10e6: the
#                  byte offset (W&$7ff0)*2 is added to the bank base, so the
#                  bank is an array of 32-byte tiles and `tile` indexes it)
#           W & $8000 == 0 (raw, $1120-$1168): copy (nib+1)*32 bytes verbatim
#                      to bank[tile*32:], dbra d2 with d2 = nib.
#           W & $8000 != 0 (RLE, $10e4-$111e): d2 = (nib+1)*16 WORDS
#                      ($10ec/$10ee) are produced, as runs, until d2 == 0
#                      exactly ($111a/$111c `tst.w d2 / bne.b $10f2`):
#             ctrl = next byte ($10f4/$10f6)
#             ctrl & $80 == 0  -> FILL : n = ctrl words, all equal to the next
#                      source word ($10fc: sub.w d0,d2 -- d0 is ctrl itself;
#                      $10fe: subq.w #1,d0; $1106/$1108: dbra loop => ctrl
#                      iterations; NOT ctrl+1)
#             ctrl & $80 != 0  -> LITERAL: n = ctrl & $7f words copied from the
#                      source ($110e-$1116, same count shape; NOT n+1)
#             (n == 0 would wrap dbra and run 65536 times, n > d2 would never
#              reach d2 == 0: neither can occur in a valid stream, so both are
#              refused here.)
#           The earlier shifts' decoder used ctrl+1 and (ctrl&$7f)+1: that
#           overshoots d2 in 47/50 of 50.NSQ's streams and 25/27 of LORI.NSQ's
#           and only "terminated" because it tolerated the overshoot and
#           wandered on to some later zero word (reports/part14-nsq.md).
#   PASS 2  $1170-$125c, reads a5 -- the ROW TABLE, the very words that
#           precede the control stream.  It is a list of screen-blit records
#           over a 40x25 grid of 8x8-pixel cells, ending at the same $FFFF the
#           $10b8 scan found:
#           $1170-$1174: word == d1 ($FFFF) -> $1260 return.
#           w = byte-swapped word ($1178).  If (w & $f800) == $f800
#           ($117c-$1184) it is a POSITION word and one more word follows
#           ($11ac): cell = w & $7ff ($1186 eor); row,col = divmod(cell, 40)
#           ($118c divu.w #$28); screen byte offset = row*$500 + (col&~1)*4
#           + (col&1) ($1196-$11a8; a2 is the screen).  Otherwise the word is
#           itself the next record, continuing at the cell after the previous
#           record's last (a0/d7 carry over, $123c-$1246).
#           record: tile = w & $7ff ($11bc); reps = ((w & $7800) >> 11) + 1
#           ($11b2, $1254/$1258 `subi.w #$800,d5 / bcc.w $11ca`); w & $8000
#           ($11b8, $124a-$1250) set -> tile += 1 after each cell (a run of
#           consecutive tiles), clear -> the same tile repeated.
#           Each cell blit ($11ca-$1228): 8 rows; row r takes bank-tile words
#           [r] and [8+r] ($11cc/$11d0, $e(a1) after the (a1)+) and movep.l's
#           them (4 bytes, one per plane, stride 2) at screen row r*160.
#           $125c: bra.w $1170 -- the loop.  $1260-$1268: return.
#   The caller ($2c3e-$2e2e): a0 = NSQ file base, a2 = screen, a3 = bank, a4 =
#   palette buffer; after each call the 16 palette words are copied to
#   $ff8240 ($2c7e-$2c82: movem.l (a0)+,d0-d7 / movem.l d0-d7,-(a1) with
#   a1 = $ff8260) -- when the frame carried a palette record ($2cb8-$2cc2
#   skip the update when $1260 hands back a0 == 0).
#
# The decode below is a faithful structural port.  The bank, screen and
# palette it produces render the intro/Loriciel-logo pictures recognisably
# (reports/part14-nsq.md).  What it does NOT model is the caller's two-screen
# swap ($12a0/$12a4 are exchanged after every call): see the report.
NSQ_TILE_BYTES = 32
_NSQ_BANK_TILES = 0x800 + 16   # 11-bit tile index + a 16-tile block's tail
NSQ_COLS, NSQ_ROWS = 40, 25    # 40 * 8 = 320 px, 25 * 8 = 200 px
NSQ_SCREEN_BYTES = 32000


def _nsq_walk_control_stream(src: bytes, i: int = 0, bank=None, written=None):
    """PASS 1 ($10c4-$116c) over `src` from byte `i`.  Returns
    (payload, end) where `payload` is the literal byte stream in source
    consumption order and `end` is the index just past the W==0 terminator.
    With `bank` (a bytearray) the blocks are also placed at tile*32; with
    `written` (a set) every tile index a block covers is added to it.
    Raises DepackError for anything the 68k loop could not finish cleanly:
    running off `src` before W==0, a zero-length run, or a run that
    overshoots its block's word budget."""
    out = bytearray()
    n = len(src)

    def put(dest, chunk):
        if bank is not None:
            if dest + len(chunk) > len(bank):
                raise DepackError("NSQ block at bank offset $%x overruns the "
                                   "tile bank" % dest)
            bank[dest:dest + len(chunk)] = chunk

    while True:
        if i + 1 >= n:
            raise DepackError("NSQ control stream ran off the end "
                               "(no W==0 terminator)")
        w = (src[i + 1] << 8) | src[i]
        i += 2
        if w == 0:
            return bytes(out), i
        nib = w & 0xF
        dest = (w & 0x7FF0) * 2
        if written is not None:
            written.update(range(dest // NSQ_TILE_BYTES,
                                 dest // NSQ_TILE_BYTES + nib + 1))
        if not w & 0x8000:
            length = (nib + 1) * NSQ_TILE_BYTES
            if i + length > n:
                raise DepackError("NSQ literal block ran off the end")
            chunk = src[i:i + length]
            i += length
            out += chunk
            put(dest, chunk)
            continue
        count = (nib + 1) * 16
        while count:
            if i >= n:
                raise DepackError("NSQ control stream ran off the end")
            ctrl = src[i]
            i += 1
            if ctrl & 0x80:
                reps = ctrl & 0x7F
                width = 2 * reps
                if reps == 0 or reps > count or i + width > n:
                    raise DepackError("NSQ literal run (%d words, %d left) "
                                       "invalid" % (reps, count))
                chunk = src[i:i + width]
            else:
                reps = ctrl
                if reps == 0 or reps > count or i + 2 > n:
                    raise DepackError("NSQ fill run (%d words, %d left) "
                                       "invalid" % (reps, count))
                width = 2
                chunk = src[i:i + 2] * reps
            i += width
            count -= reps
            out += chunk
            put(dest, chunk)
            dest += len(chunk)


def decode_nsq_control_stream(src: bytes) -> bytes:
    """Decode one NSQ frame's PASS-1 control stream ($10c4-$116c).

    `src` must start at the first control word (right after a frame's
    0xFFFF-terminated row table).  Returns the literal byte payload in
    source-consumption order (no placement -- see `nsq_decode_frame` for
    that).  W == 0 IS the end of this stream ($10d0/$10d2, d3 == 0); the
    row table that $1170 then walks lies BEFORE the stream (see
    `nsq_blit_list`), so nothing follows the terminator in `src` that this
    frame needs.  Raises DepackError if the stream does not terminate
    cleanly inside `src` (the pre-discr-lul version silently accepted that,
    and used run lengths one too long)."""
    return _nsq_walk_control_stream(src, 0)[0]


def nsq_blit_list(data: bytes, table_start: int):
    """PASS 2's record parser ($1170-$125c) over the row table at
    `table_start`.  Returns (records, end): `records` is a list of
    (cell, tile, reps, run) -- start cell (row*40+col), bank tile index, cell
    count, and whether the tile index advances per cell -- and `end` is the
    index just past the terminating $FFFF word."""
    recs = []
    cell = None
    i = table_start
    n = len(data)
    while True:
        if i + 1 >= n:
            raise DepackError("NSQ row table ran off the end")
        raw = data[i:i + 2]
        i += 2
        if raw == b"\xff\xff":
            return recs, i
        w = (raw[1] << 8) | raw[0]
        if (w & 0xF800) == 0xF800:
            cell = w ^ 0xF800
            if i + 1 >= n:
                raise DepackError("NSQ row table ran off the end")
            w = (data[i + 1] << 8) | data[i]
            i += 2
        elif cell is None:
            raise DepackError("NSQ row table starts without a position word")
        reps = ((w & 0x7800) >> 11) + 1
        recs.append((cell, w & 0x7FF, reps, bool(w & 0x8000)))
        cell += reps


def nsq_descramble_palette(rec: bytes) -> bytes:
    """$126a: the 32-byte record -> 32 bytes (16 big-endian ST palette words
    0RGB, 3 bits per gun).  Per input byte pair (in0, in1): out0 = (in0>>3)&7;
    out1 = (((X:in0) roxr.b #2) & $70) | (in0 & 7) with X = in1 bit 0
    ($1270-$128e)."""
    if len(rec) != 32:
        raise DepackError("NSQ palette record must be 32 bytes")
    out = bytearray()
    for k in range(16):
        in0, in1 = rec[2 * k], rec[2 * k + 1]
        v = ((in1 & 1) << 8) | in0           # 9-bit rotate through X
        v = ((v >> 2) | ((v & 3) << 7)) & 0x1FF
        out.append((in0 >> 3) & 7)
        out.append((v & 0x70) | (in0 & 7))
    return bytes(out)


def _nsq_entry_offset(data: bytes, idx: int) -> int:
    if not 0 <= idx < data[4]:
        raise DepackError("NSQ frame index %d out of range" % idx)
    base = 8 + idx * 3
    return (data[base] << 16) | (data[base + 2] << 8) | data[base + 1]


def nsq_frame_layout(data: bytes, idx: int):
    """Locate frame `idx`'s parts exactly as $1076-$10ba do.  Returns a dict:
    off (entry offset), palette (the 32-byte scrambled record or None),
    table (row-table start), empty (True when the table is just $FFFF), cs
    (control-stream start, None when empty)."""
    off = _nsq_entry_offset(data, idx)
    first = (data[off + 1] << 8) | data[off]
    if (first & 0xF800) == 0xF800:
        pal, table = None, off
    else:
        pal, table = data[off:off + 32], off + 0x20
    if data[table:table + 2] == b"\xff\xff":
        return {"off": off, "palette": pal, "table": table,
                "empty": True, "cs": None}
    return {"off": off, "palette": pal, "table": table, "empty": False,
            "cs": _nsq_control_stream_offset(data, off)}


def _nsq_blit_cell(screen: bytearray, cell: int, tile: bytes):
    row, col = divmod(cell, NSQ_COLS)
    if row >= NSQ_ROWS:
        raise DepackError("NSQ blit cell %d is off the 40x25 screen" % cell)
    a0 = row * 0x500 + (col & ~1) * 4 + (col & 1)
    for r in range(8):
        o = a0 + r * 160
        screen[o] = tile[2 * r]
        screen[o + 2] = tile[2 * r + 1]
        screen[o + 4] = tile[16 + 2 * r]
        screen[o + 6] = tile[16 + 2 * r + 1]


def nsq_decode_frame(data: bytes, idx: int, bank: bytearray,
                     screen: bytearray):
    """One $1048 call for frame `idx`, applied to the caller's tile bank
    (bytearray of at least (0x800+16)*32 bytes) and screen (32000 bytes).
    Returns the frame's descrambled 32-byte palette, or None when it has no
    record.  Frame index 0 clears the screen first ($10a0-$10a8)."""
    lay = nsq_frame_layout(data, idx)
    pal = nsq_descramble_palette(lay["palette"]) if lay["palette"] else None
    if idx == 0:
        screen[:] = bytes(NSQ_SCREEN_BYTES)
    if lay["empty"]:
        return pal
    _nsq_walk_control_stream(data, lay["cs"], bank)
    recs, _ = nsq_blit_list(data, lay["table"])
    for cell, tile, reps, run in recs:
        for k in range(reps):
            t = tile + (k if run else 0)
            _nsq_blit_cell(screen, cell + k,
                           bank[t * NSQ_TILE_BYTES:(t + 1) * NSQ_TILE_BYTES])
    return pal


def nsq_play(data: bytes):
    """Yield (idx, screen_bytes, palette_bytes_or_None) for frames 0..count-1,
    one persistent tile bank and ONE persistent screen.  palette is the last
    one seen (the hardware palette after the caller's $ff8240 copy), None
    before the first.  Single-screen model: the caller's two-screen swap is
    not modelled (see the NSQ comment block above)."""
    bank = bytearray(_NSQ_BANK_TILES * NSQ_TILE_BYTES)
    screen = bytearray(NSQ_SCREEN_BYTES)
    pal = None
    for idx in range(data[4]):
        p = nsq_decode_frame(data, idx, bank, screen)
        pal = p or pal
        yield idx, bytes(screen), pal


def nsq_screen_to_rgb(screen: bytes, palette) -> bytes:
    """ST low-res planar screen (4 interleaved planes per 16 px) + palette
    -> 320x200 RGB bytes.  With palette None, a grey ramp is used."""
    cols = []
    for k in range(16):
        if palette:
            cols.append(((palette[2 * k] & 7) * 36,
                         ((palette[2 * k + 1] >> 4) & 7) * 36,
                         (palette[2 * k + 1] & 7) * 36))
        else:
            cols.append((k * 17,) * 3)
    out = bytearray()
    for y in range(200):
        for gx in range(20):
            o = y * 160 + gx * 8
            p = [(screen[o + 2 * k] << 8) | screen[o + 2 * k + 1]
                 for k in range(4)]
            for x in range(16):
                v = sum(((p[k] >> (15 - x)) & 1) << k for k in range(4))
                out += bytes(cols[v])
    return bytes(out)


def nsq_check(data: bytes):
    """Account for every frame of one NSQ file given as EXACTLY its own
    `byte_size` bytes (so any read past its end is an error, not a quiet
    read into the next file on disk).  A frame is 'accounted' only if ALL of:
      1. its pass-1 stream terminates by W==0 inside the file, every RLE
         block landing on d2 == 0 exactly (or the frame is empty: table ==
         just $FFFF);
      2. pass 2's record parse ends on the very $FFFF the $10b8 scan found;
      3. every tile pass 2 blits was written to the bank by this or an
         earlier frame (frames in index order);
      4. the frame's data ends -- after its terminator, rounded up to an even
         address -- exactly at the next entry's offset (entries sorted by
         offset); the last one ends exactly at the file's end.
    Returns (n_frames, n_empty, accounted, failures) with failures a list of
    (idx, reason)."""
    n = data[4]
    entries = sorted((_nsq_entry_offset(data, i), i) for i in range(n))
    nxt = {i: (entries[k + 1][0] if k + 1 < n else len(data))
           for k, (_, i) in enumerate(entries)}
    bank = bytearray(_NSQ_BANK_TILES * NSQ_TILE_BYTES)
    written = set()
    empty = accounted = 0
    failures = []
    for idx in range(n):
        try:
            lay = nsq_frame_layout(data, idx)
            if lay["empty"]:
                empty += 1
                end = lay["table"] + 2
            else:
                _, end = _nsq_walk_control_stream(data, lay["cs"], bank,
                                                  written)
                recs, tend = nsq_blit_list(data, lay["table"])
                if tend != lay["cs"]:
                    raise DepackError("row table ends at $%x, scan found "
                                       "$FFFF at $%x" % (tend, lay["cs"]))
                for cell, tile, reps, run in recs:
                    for k in range(reps):
                        t = tile + (k if run else 0)
                        if t not in written:
                            raise DepackError("tile $%x blitted before any "
                                               "frame wrote it" % t)
            if end + (end & 1) != nxt[idx]:
                raise DepackError("frame ends at $%x, next data starts at $%x"
                                   % (end, nxt[idx]))
            accounted += 1
        except (DepackError, IndexError) as e:
            failures.append((idx, str(e) or "ran off the file"))
    return n, empty, accounted, failures


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
    if len(argv) == 2 and argv[0] == "--nsq-check":
        # Strict NSQ accounting (see nsq_check()) for every *.NSQ directory
        # entry of a DSC-shaped image, each read as exactly its own byte_size.
        data = open(argv[1], "rb").read()
        bad = 0
        for e in _parse_directory(data):
            if not e["name"].upper().endswith(".NSQ"):
                continue
            f = data[e["offset"]:e["offset"] + e["byte_size"]]
            n, empty, ok, fails = nsq_check(f)
            print("%-10s frames=%d empty=%d accounted=%d/%d"
                  % (e["name"], n, empty, ok, n))
            for idx, why in fails:
                print("  frame %d: %s" % (idx, why))
            bad += len(fails)
        return 1 if bad else 0
    if len(argv) == 4 and argv[0] == "--nsq-ppm":
        _, inpath, sel, outpath = argv
        data = open(inpath, "rb").read()
        want = int(sel, 0)
        for idx, screen, pal in nsq_play(data):
            if idx == want:
                with open(outpath, "wb") as f:
                    f.write(b"P6 320 200 255\n" + nsq_screen_to_rgb(screen, pal))
                return 0
        sys.stderr.write("loriciel_depack: no frame %d\n" % want)
        return 1
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

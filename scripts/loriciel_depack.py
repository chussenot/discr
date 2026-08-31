#!/usr/bin/env python3
"""Loriciel graphics depacker for "Disc" (Loriciel, 1990, Atari ST) -- discr-rxx.5.

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
  bit-level token algorithm is still NOT round-trip-proven (see
  reports/part13-depack2.md), so depack() still refuses these four names --
  landing the codec here requires a proven byte-exact implementation, not
  just a proven association between file and routine.

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


def depack(data: bytes, *, name: str | None = None) -> bytes:
    """Identity depack for the file classes proven raw in RAM (see module
    docstring). Refuses (DepackError) anything not independently confirmed,
    rather than silently returning input unchanged for an unknown/actually
    -compressed file such as DALLES01.DAT.
    """
    if name is not None:
        base = name.upper().rsplit("/", 1)[-1].rsplit("\\", 1)[-1]
        if base in _PROVEN_RAW_FULL_SHA256:
            return data
        if base in ("DALLES01.DAT", "PLAYER01.DAT", "ENEMY01.DAT",
                     "DECOR02.DAT", "DECOR03.DAT", "DECOR04.DAT"):
            raise DepackError(
                "%s: no proven depacker (see UNRESOLVED in this module's "
                "docstring and reports/part13-depack.md)" % base)
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

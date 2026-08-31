# Part 13, second shift — the "Ice!" routine is real and it IS the game's depacker

Second-shift pass on discr-rxx.5, following `reports/part13-depack.md`
(first shift). Headline: the low-memory "Ice!"-class routine Part 13 landed
as "role unconfirmed... never fired" **does fire**, 32 times, across a
completely ordinary boot→menu→CHALLENGE-match session — and three of the
first shift's four still-unresolved graphics files (`DALLES01.DAT`,
`PLAYER01.DAT`, `ENEMY01.DAT`) are proven, live, to go through it. The
bit-exact codec is **not** round-trip-proven this shift (see "What's still
open" below) — per house rules, `scripts/loriciel_depack.py` still refuses
these files rather than guess. `PROGRAM.HA`'s question is narrowed from "who
writes `$a4ea`" to "who writes `$42506`": the `$a4ea` writer is a plain
relocator, caught red-handed copying already-correct bytes.

## Method

Same two instruments the first shift established: `scripts/collect.py`'s
`Hatari` class (control-socket debugger, PC/write breakpoints with `:trace
:lock` so the emulator never drops into an unattended prompt) driven from
short standalone Python scripts (not committed — they lived in the session's
scratch directory), plus `:file` actions to capture full CPU register state
and memory windows AT the exact instant a breakpoint fires, the same pattern
`Hatari.seed()` already uses. Sessions used `navigate_to_match(mode=
"challenge")` for a real bout, per `KNOWN_ISSUES.md`.

One new technique this shift needed and used: **compound breakpoint
conditions** (`b pc = $30c && a2 = $a4ea :once :trace :file <script>`) to
land on one exact instant inside a hot per-byte copy loop without
runaway-tracing every iteration. First attempt at this (`history cpu` plus a
raw `m l` memdump inside a `:file` script) produced a 26-million-line log in
under 90 seconds before the process was killed — landed here as a new
KNOWN_ISSUES-class caution: **never combine `history cpu` with a `:file`
action on a breakpoint that can fire more than a handful of times**; it does
not behave like the same command run interactively.

## Finding A: `$320` ("Ice!" check) fires 32 times; 21 succeed

The first shift's own suggested follow-up (`b pc = $320 :trace :lock :file
<script>`, capturing full registers on every hit across a full boot→menu→
match session) was run. Contrary to `reports/part13-depack.md`'s "never
fired across boot→menu→match": **32 hits**, first at VBL 871-874 (varies a
few frames run to run), last at VBL ~17700-17900 (during match-entry
loading). D0 holds the just-read 4-byte magic at every hit; only 21 of 32
equal `$49636521` ("Ice!") — the other 11 are genuine misses (`HABS`,
`NSEQ`, and a couple of non-ASCII numeric values), confirming `$320` is a
**shared per-file format prober** called once for every module the loader
touches, not something specific to one file.

A cold-boot probe (arming `b pc = $320` as the very first command after
connect, VBL 4-6) confirmed the routine's own code is **not present yet** at
that point — `disasm $300 $340` reads as uninitialized fill, not the
decoder. A write-watch on `($320).w` caught the installer: **one** write
event at VBL 837, writer PC `$3f012` (`dbf.w d0,#$fffc` — the tail of a
word-copy loop, so the actual mover is just above `$3f010`). `$3f012` sits
right next to the `$3F000` address the bead's own notes already named as
something the DSC bootstrap references — this is that bootstrap relocating
its own low-memory runtime (HABS loader + Ice! decoder together) out of a
staging area near `$3F000` down to `$300`-`$800`, in one pass, early in
boot.

## Finding B: three of the four unresolved `.DAT` files ARE Ice!-depacked — proven by header size match

At every `$320` hit, `A0` (source cursor) has already consumed the 4-byte
magic; `A0-4` is the header start. Live disasm of `$2e0`-`$500` (fresh,
executing code — not the stale, superseded bytes the first shift's
post-session disasm risked) gives the exact header consumption order:

```
$31a  movem.l d1-d7/a0-a6,-(a7)        ; save
$31e  bsr $37e                         ; read 4 bytes (a0)+ -> d0 = magic
$320  cmp.l #$49636521,d0              ; "Ice!"
$326  bne $376                         ; fail: restore, d0=$ea, rts
$328  bsr $37e                         ; read 4 bytes -> d0 = P  (header+4)
$32a  lea (-8,a0,d0.l),a5              ; a5 = header_start + P  (bit-reader end)
$32e  bsr $37e                         ; read 4 bytes -> d0 = Q  (header+8)
$330  move.l d0,(a7)                   ; stash Q
$332  movea.l a1,a4                    ; a4 = dest base (a1 supplied by caller)
$334  movea.l a1,a6
$336  adda.l d0,a6                     ; a6 = dest base + Q  (dest end)
$338  movea.l a6,a3
$33a  bsr $3b6 / $33c bsr $38a / ...   ; token loop (see Finding D)
```

`$37e`-`$388` is the shared "read 4 bytes big-endian from (a0)+" helper —
confirms the earlier live capture where D0 read `48414253` ("HABS") for a
call that then correctly failed the `$320` compare (that call's file wasn't
Ice!-packed; the same dispatcher clearly tries every file against every
format tester it has).

Capturing `A1` (destination) and the 12-byte header (magic, P, Q) at every
one of the 21 successful hits, across a full boot→menu→CHALLENGE-match
session:

| VBL | dest (A1) | header P (offset+4) | header Q (offset+8) | P matches |
|---|---|---|---|---|
| 12832 | `$1D41C` | **12500** | 31664 | `DALLES01.DAT` (12500 B) exactly |
| 13014 | `$24FCC` | **67211** | 152360 | `PLAYER01.DAT` (67211 B) exactly |
| 13253 | `$4A2F4` | **28785** | 63070 | `ENEMY01.DAT` (28785 B) exactly |
| 12744 | `$156FC` | **7477** | 32032 | `DECOR00.DAT` (7477 B) — second, independent load |
| (17 more) | various | various | various | LUTs, sprite masks, a 37260 B UI graphic (menu portraits, reused 3x) — not among the bead's named targets |

This is the decisive proof-pair the first shift's own "suggested follow-up"
asked for: **`P` (the header's first size field) equals the target file's
exact on-disk byte count**, for three of the four still-open files plus a
second, independent load of a file already proven raw by a completely
different mechanism (the plain HABS-style byte copy, Finding 1 of the first
report). `DECOR02/03/04.DAT` were not observed loading at all this session
— neither raw nor via Ice! — almost certainly because CHALLENGE mode this
scenario ran picked a court/decor skin that doesn't use them, not because of
anything about their codec. A future pass needs a scenario that selects a
different arena.

This settles the ownership question the first shift left open: the "Ice!"
routine executes as ordinary, repeated, load-bearing code during a **plain
boot of the cracked floppy image** (not merely present in PP's separate
`RUNME.TOS` HD-adaptation wrapper), and it decompresses the game's own named
target assets. It is Loriciel's (or at least this release's) own graphics
depacker, exercised on every normal playthrough — not dead code, not a
leftover shared-library branch, and not something PP's tooling introduced.

## Finding C: `$25c2` is a red herring — an interrupt handler, not a loader

One of the first shift's four candidate writer-PCs for `$a4ea` (Finding 3's
closing paragraph) was `$25c2`, flagged as unresolved because its
*file*-static bytes read as padding. A fresh **live** disasm of
`$2560`-`$2680` (captured well after boot, so the code actually resident
there is current, not stale) shows it is real, coherent code — but it is an
**interrupt handler**: `$25c2` starts with `movem.l d1-d3/d6-d7/a2-a4,-(a7)`
and the routine runs to `$26fe: rte` (return-from-exception). It does
`movep` writes into `$10F00` (the same NSQ canvas destination address
Finding 2 of the first report named) and touches several other tables —
consistent with a periodic (VBL- or timer-driven) sprite/canvas updater, not
any kind of file loader. It writes `$a4ea` only incidentally, as a
side-effect of its own unrelated per-frame work. Ruled out.

`$11fc` and `$103a` remain ruled out as the first shift found (incidental
`LAUNCHER.HA` blitter/memory-clear code, reused-by-address before
`PROGRAM.HA`'s real home is claimed).

## Finding D: `$30c` is a pure relocator — PROGRAM.HA's real depack step is earlier and still open

The fourth candidate, `$30c`, sits inside the **generic HABS-style loader**
(`$2ec`-`$316`, first shift's Finding 3, first bullet) — but live disasm
this shift shows it is not a simple single-blob byte copy. It is a
**segmented, self-relocating copy**:

```
$2ec  cmp.l #$48414253,(a1)       ; "HABS"?
$2f4  movea.l (8,a1),a0           ; a0 = module entry point
$2f8  move.l (0x10,a1),d0         ; d0 = segment count
$2fc  lea.l (0x14,a1),a1          ; a1 = start of segment-descriptor stream
$300  move.l (a1)+,d1             ; d1 = this segment's byte count
$302  movea.l (a1)+,a2            ; a2 = this segment's OWN destination
$304  btst.l #$1f,d1 / $308 bne $310
$30a  move.b (a1)+,(a2)+          ; byte copy loop
$30c  subq.l #1,d1
$30e  bne $30a
$310  subq.l #1,d0
$312  bne $300                    ; next segment
$314  jmp (a0)                    ; done: jump to entry point
```

Each segment carries its OWN destination pointer inside the data stream —
this is a real multi-segment relocating loader, matching a HABS-style
absolute-fixup format even without every module needing the literal `HABS`
ASCII check (a caller can jump straight into `$2f4`/`$300` already knowing
the format).

A compound conditional breakpoint, `b pc = $30c && a2 = $a4ea :once :trace
:file <script>`, landed exactly once, mid-`navigate_to_match`, with full
registers:

```
D0=00000003  D1=0000B213  D2=008800A1  D3=00007EA1
A0=00008000  A1=00042506  A2=0000A4EA  A3=00012F00
```

`A1 = $42506` is the source, `A2 = $a4ea` the destination — this is the
transformer's own hand caught on `$a4ea`. Saving a 32 KB window at the
source and an 8 KB window at the destination **at that exact instant**
(`savebin` inside the same `:file` script) settles the question decisively:

```
bytes at A1 (source, live): 4bf86e3e76074a2d00106700021a78ff226d003e45e9...
known ground truth $a4ea:   4bf86e3e76074a2d...
```

**The source bytes are already the final, correct resident code**,
byte-for-byte, before this copy loop even runs. `$30c`/`$2ec` is therefore a
**pure relocator** — it moves already-depacked bytes to their execution
home; it performs no transformation of its own. This settles the bead's own
open hypothesis in favour of "relocator", for this specific copy step.

But that pushes the real question one level earlier: **what wrote the
correct bytes to `$42506`?** Two things are now ruled out:

- It is **not** `$2E512` (the first shift's proven-raw `PROGRAM.HA` staging
  copy) — `$42506`'s content never appears anywhere in
  `assets/original/PROGRAM.HA`'s raw bytes (checked both a direct
  offset-anchored search and a sliding 64-byte-window search across the
  whole captured 32 KB source region — no match anywhere). `$2E512`'s copy
  looks like a discarded prefetch, not the resident build's real input.
- It is **not** one of the 21 captured `$320` (Ice!) successes — none of
  their destinations (`A1` in Finding B's table) is anywhere near `$42506`,
  and `$42506` sits inside `DISC.ALL`'s documented aliasing span
  (docs/loriciel-formats.md §6, `$3600`-`$50C00`), consistent with it being
  a `DISC.ALL`-relative buffer distinct from any of the per-file Ice! loads
  observed.

So the actual `PROGRAM.HA` depack step happens strictly before the `$30c`
relocation, writes its output somewhere that lands at/around `$42506`, and
was not caught directly in this pass. Given Finding B's proof that the same
Ice! machinery is genuinely load-bearing for this build, the most likely
next move is a wider net: watch `($42506).l` (or a range around it) for
writes across the same boot→match window, or re-run the `$320` capture with
a **longer** post-menu tail (this pass's last captured Ice! hit was at VBL
~17700-17900, right at match entry — a `PROGRAM.HA`-sized decompression
could plausibly be timed slightly later or slightly earlier than this
session's capture window caught).

## What's still open (landed as annotated partial, not guessed into the Python)

1. **The bit-exact `.DAT` codec.** Finding B proves DALLES01/PLAYER01/
   ENEMY01 go through the Ice!-class routine, but not its bit-level
   semantics. Two structural facts complicate the "it's just Pack-Ice LZ"
   read the first shift landed:
   - Header field `P` (used to compute `A5 = header_start + P`, the
     backward bit-reader's starting point) is suspiciously close to
     `12 (header) + original_file_size` for all three files — consistent
     with these particular payloads being **stored**, not compressed
     (near-zero size reduction), inside the Ice! container.
   - Header field `Q` (used to size the destination, `A6 = dest + Q`) is
     consistently **2.2-2.5x larger** than `P` across all three files
     (31664/12500=2.53, 152360/67211=2.27, 63070/28785=2.19) — a
     suspiciously *consistent* ratio for something that should vary with
     data redundancy if it were pure LZ expansion. The live disasm's
     `$344`-`$36a` block (traced fresh this shift, not from the first
     shift's possibly-stale read) is a 4-way bit-scatter accumulator
     (`add.w d4,d4` / `addx.w dN,dN` chains across `d0`-`d3`) ending in a
     single `movem.w d0-d3,(a3)` — structurally a chunky/packed-nibble to
     ST-native-4-bitplane format converter, the same idiom demos use for
     chunky-to-planar, not a generic LZ token replay.
   - Directly diffing the decompressed RAM bytes at each destination
     against the source `.DAT` file (byte-for-byte at the front, and a
     sliding literal search over the first several KB) found **zero**
     relationship at the raw byte level — ruling out "it's the same bytes,
     just reordered by a fixed permutation" as a shortcut, and confirming
     real reformatting is happening, not a trivial transform. Round-tripping
     this needs either full instruction-level tracing of one complete
     decompression (a savestate + single-step run through one whole
     invocation, watching every write) or picking apart the remaining
     `$38a` dispatch branches (Finding 3 of the first report already named
     four: `$3aa` uncompressed, `$394` 2-bit-prefixed table, `$416`-`$476`
     unary length/distance, `$456` reduced-range) against this
     now-corrected live disasm.
2. **`PROGRAM.HA`'s real depack step** (writer of `$42506`, per Finding D)
   — not caught this pass.
3. **`DECOR02/03/04.DAT`** — not observed loading (raw or Ice!) in a
   CHALLENGE match this session; needs a scenario that selects a different
   court/decor skin.

Per house rules, none of this is landed as a codec in
`scripts/loriciel_depack.py` — the module's refusal behaviour for
`DALLES01.DAT`/`PLAYER01.DAT`/`ENEMY01.DAT`/`DECOR02-04.DAT` is unchanged,
and the docstring is updated with everything above (exact addresses,
registers, and the corrected live disasm) so the next pass starts from
proof, not from Finding 3's superseded "never fired" read.

## Housekeeping: `_nsq_frames()` stride bug (flagged by `formats`, discr-rxx.6)

While this file was leased, `formats` sent a pact message (not editing my
file, correctly addressing me instead) reporting a bug in `_nsq_frames()`:
the entry stride was 6 bytes, should be 3 (`mulu.w #3,d0` at LAUNCHER.HA's
`$105c`), and each entry's 3-byte value is a **file offset to a 32-byte
record**, not the control-stream start directly — the record needs a
`0xf800` mask check on its first byte-swapped word to know whether to skip
0 or 0x20 bytes before scanning for the row-table's `0xFFFF` terminator,
after which the control-word stream starts.

Fixed both (stride, and a new `_nsq_control_stream_offset()` implementing
the record-skip + terminator scan) per their description. Own light
validation (decode-without-erroring, not full byte-correctness against live
DSC RAM the way `formats` validated theirs) against the real
`50.NSQ`/`LORI.NSQ` files: **38/50** and **31/36** frames decode without
error — better than the un-fixed stride-6 version (which computed wrong
offsets for nearly every frame) but short of `formats`' own reported
50/50 and 27/36, most likely because `decode_nsq_control_stream()`'s own
docstring already flags an incomplete piece: it only implements the `$10c4`
branch and explicitly does not follow the `$1170` "next block" continuation
for a `W==0` control word, so any frame whose stream spans more than one
block will hit "ran off the end" here regardless of the offset fix. Landed
as an honest partial improvement, not claimed as matching `formats`' fuller,
ground-truth-validated fix in `docs/loriciel-formats.md` §4 /
`reports/part13-formats.md`.

## Validation

- `python3 -m py_compile scripts/loriciel_depack.py` — clean.
- Round-trip re-check of the three already-proven files (unchanged from the
  first shift, re-run this shift as a regression check):
  `python3 scripts/loriciel_depack.py assets/original/DECOR00.DAT /tmp/out.bin`
  (and `BONUS01.DAT`, `VIC.DAT`) — sha256 of each output matches the first
  shift's report exactly (`5b739722d4b12f57...`, `8e644eb0d1de084b...`,
  `dcca430cfc90a62e...`).
- Confirmed the module still refuses `DALLES01.DAT` (`DepackError`, exit 1).
- `cargo test -p disc-core` — `59 passed; 0 failed` (unit tests) +
  `1 passed` (`anim_measure.rs`) — tree sanity, no regression from this
  shift's (Python/docs/report-only) changes.

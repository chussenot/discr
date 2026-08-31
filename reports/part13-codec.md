# Part 13, third shift — the Ice! codec is pinned, bit-exact, 4/4 round-trip

Third-shift pass on discr-rxx.5, following `reports/part13-depack.md` (first
shift) and `reports/part13-depack2.md` (second shift). Headline: the Ice!
bitstream format the second shift proved was DALLES01.DAT/PLAYER01.DAT/
ENEMY01.DAT/DECOR00.DAT's (second load) depacker, but left un-round-tripped,
is now fully reverse-engineered and implemented as `depack_ice()` in
`scripts/loriciel_depack.py`. All four proof pairs decode byte-identical to
live Hatari RAM (sha256-checked). The catch, and it's a real one: the ACTUAL
bytes the depacker reads for these four files are **not**
`assets/original/DALLES01.DAT` (etc.) — those on-disk bytes are proven to be
a different byte sequence from what the routine consumes at runtime — so
`loriciel_depack.py`'s `depack()` still refuses these four names when given
the plain on-disk file, and only decodes the live-captured container blob.
PROGRAM.HA's transform remains open (filed as a follow-up below). The NSQ
harness discrepancy has a definitive root cause, address-cited, though not a
clean "which number was right" answer — see the closing section.

## Method

Same two-layer approach as the prior two shifts, `scripts/collect.py`'s
`Hatari` class (control-socket debugger), used in a new way this shift:
**interleaved multi-breakpoint sequential tracing**. Single-stepping
(`step`) inside a `:trace` breakpoint's `:file` script turned out to be a
no-op (confirmed empirically: 220 consecutive `step`+`r` pairs all reported
the identical PC and registers) — `:trace` never actually suspends the CPU,
so there's nothing for `step` to advance from. The working substitute: arm
several *non-*`:once` `:trace :lock` breakpoints on different addresses
inside one basic-block sequence simultaneously, scoped by a stable register
condition (`a4==<dest>`, constant for a whole file's decode), and read the
hits back in the order they appear in the shared log — this reconstructs a
true instruction-level trace for the addresses of interest without ever
needing the CPU to actually stop.

`savebin`'s address argument does **not** accept expressions (`a5-P-256`
fails with "contains non-alphanumeric characters" — confirmed live); only a
bare register name, symbol, or literal number. Since this session's boot is
fully deterministic (same disk image, same scripted input timing → identical
register values across independent Hatari process launches, confirmed
repeatedly), the fix is to capture the needed address once via `r`, then
issue a second `savebin` with that literal hex value substituted in.

## Finding 1: the Ice! codec, fully derived and round-trip-proven

### Header

A 12-byte header — `b"Ice!"` (4B) + big-endian `u32 P` (4B) + big-endian
`u32 Q` (4B) — precedes the packed payload. **`P` is measured from the
header's own start**, i.e. `P` = 12 (header) + payload length, not just the
payload length (confirmed: the backward bit-reader's start pointer
`a5 = header_address + P`, and live capture shows the header genuinely sits
at index 0 of that P-byte span — `window[:12] == b"Ice!" + P.to_bytes(4,
"big") + Q.to_bytes(4, "big")` for all four captures). `Q` is the exact
depacked output size.

### Bit reader ($3c2/$3e2/$408, and the one-time primer at $3b6)

A 32-bit `d7` register, MSB-first, refilled by reading 4 bytes **backward**
from a moving cursor (`a5`) when exhausted. The exhaustion signal is a
sentinel bit: each refill ORs a `1` into the new word's LSB (`(word<<1)|1`),
so repeated `d7 <<= 1` extractions eventually shift that sentinel up to
bit 31 and pop it as a (garbage) "bit", producing `d7==0` — the refill path
then reads 4 fresh bytes, returns their MSB as the *real* bit for this call
(the garbage bit is silently discarded), and re-plants the sentinel.  The
one-time primer at `$3b6` (used once, right before the main loop) has no
sentinel — it's a plain byte-order-obscured 4-byte backward read
(`4×(move.b -(a5),d7 / ror.l #8,d7)`, verified to net out to the same value
`move.l -(a5),d7` would give — the four instructions just exist to dodge
the 68000's word-alignment requirement when `a5` is odd).

```python
def get_bit(self):
    shifted = self.d7 << 1
    bit = (shifted >> 32) & 1
    self.d7 = shifted & 0xFFFFFFFF
    if self.d7 == 0:                      # sentinel just consumed -> refill
        self.a5 -= 4
        neww = int.from_bytes(self.src[self.a5:self.a5+4], "big")
        bit = (neww >> 31) & 1
        self.d7 = ((neww << 1) | 1) & 0xFFFFFFFF
    return bit
```

### The DBcc/DBNE polarity trap

This is the one piece that static disassembly alone left genuinely
ambiguous, and got wrong on a first pass. Real 68000 `DBcc`: *"if condition
is FALSE, decrement and branch; if condition is TRUE, fall through
unchanged"* — the loop **continues** on the condition being **false**, the
opposite of how a same-named `Bcc` reads. For `$3a2`'s `dbne.w d3,$39a`
(the escalating literal-length table search, `$394`-`$3aa`): the loop
continues (retries with the next, wider candidate) when the just-read bits
**equal** the candidate's target value, and *stops* (accepts this
candidate) the moment they **don't**. Verified two ways: (1) live
interleaved tracing of `pc=$39a`/`pc=$3a2`/`pc=$3a6` scoped by
`a4==$1D41C` across DALLES01.DAT's first several tokens, reading the exact
`D0`/`D1`/`a1` at each step; (2) the resulting Python decoder round-trips
all four files byte-for-byte, which a flipped polarity could not
coincidentally produce at this scale (152,360 bytes for PLAYER01.DAT).
Same polarity for `$41e`/`$440`'s `dbcc` (length/distance unary prefixes):
loop continues on bit==1, stops on bit==0.

### Token loop ($38a-$476)

```
bit0 = get_bit()
if bit0 == 0:
    literal_len = None                       # no literal this token
else:
    bit1 = get_bit()
    literal_len = 0 if bit1 == 0 else literal_extra_length()   # see $394 below
if literal_len is not None:
    copy (literal_len + 1) bytes, source (a5)-- backward, dest (a6)-- backward
    if output full: done
d4 = decode_length()               # $416-$436, see length table below
distance = (decode_reduced_distance() if d4 == 0     # $456, d4==0 special case
            else decode_distance())                  # $438-$454
match_len = d4 + 2
src = (current write position) + 2 + d4 + distance
copy match_len bytes backward from src into the output (self-referential OK)
if output full: done, else loop back to bit0
```

**Escalating literal-length table** (`$394`-`$3a6`, PC-relative table at
`$48e`, 5 long-words, walked via 4-byte-predecrement reads starting at
`$48e`, i.e. reads land at `$48a`,`$486`,`$482`,`$47e`,`$47a` in that
order — a second, parallel table at `+0x14` from each read position holds
the additive base, which coincidentally lands on the SAME 10 bytes the
length table (`$4a2`) uses, just addressed differently):

| candidate | read (nbits+1) | target (equal → keep searching) | +base |
|---|---|---|---|
| 0 (`$48a`) | 2 bits | 3 | 1 |
| 1 (`$486`) | 2 bits | 3 | 4 |
| 2 (`$482`) | 3 bits | 7 | 7 |
| 3 (`$47e`) | 8 bits | 255 | 14 |
| 4 (`$47a`) | 15 bits | 32767 | 269 |

If all five candidates read exactly their target (astronomically unlikely
for real data), the DBNE counter itself exhausts and candidate 4 is forced.

**Length table** (`$4a2`, 10 bytes, exactly as the second shift extracted:
`09 01 00 ff ff 08 04 02 01 00`) and **distance table** (`$4ac`, 10 bytes:
`0b 04 07 00 01 20 00 00 00 20`) — both `{signed extrabits[5 or 3] : i8,
baselen[5 or 3] : u8-or-u16}`, indexed by a unary-prefix count (`$416`-
`$41e` for length, 4 max continuations, index range -1..3; `$438`-`$440`
for distance, 2 max continuations, index range -1..1). `baselen` is a byte
array for length, a **word** array for distance (confirmed live: `$450`'s
`add.w $6(a1,d2.w),d1` uses `d2` pre-doubled). A negative `extrabits`
(length only, indices 2 and 3 — `0xff` as `i8` = -1) means "skip the
extra-bits read entirely, extra value is 0" — this is also the
zero-continuation, immediate-stop case (`d2==3`) exactly when it yields
`baselen[3]==0`, which triggers the special reduced-distance path below.

**Reduced-distance path** (`$456`-`$466`, only reached when
`decode_length()` returns `d4==0`): one selector bit picks a plain 6-bit
distance (0-63) or a 9-bit distance biased by `+64` (64-575), with an
implicit match length of exactly 2 (`d4+2`).

**Match copy**: `source_index = current_write_position + 2 + d4 + distance`,
then `match_len = d4+2` bytes copied one at a time, predecrementing both
source and dest — since `source_index` is always ≥ the position just
written, this is a legal self-referential LZ copy (it can read bytes the
SAME match is still in the process of writing, correctly, byte at a time).

### Post-process: chunky4→planar4 bit-scatter ($344-$36c)

Right after the main token loop finishes (destination fully written), one
more bit is read; if 1, a fixed 4000-iteration, 8-bytes-per-iteration
(32,000-byte) transform runs over the **tail** of the output buffer
(walking backward from the very end), rewriting each 8-byte chunk (4 words)
by extracting 4 bits from each word per pass and interleaving them 4-ways
across the 4 destination words — a textbook chunky-to-bitplane conversion,
consistent with a 320×200×4bpp low-res screen convention (32,000 =
320×200×4/8). Confirmed live: PLAYER01/ENEMY01/DALLES01 all read this bit
as 0 (skip); DECOR00's second load also reads 0 in practice (its Q=32,032
is suspiciously close to 32,000 but the extra bit is genuinely 0 for this
file — implemented and tested, but not exercised by any of the four proof
pairs beyond "reads correctly as 0").

### The four proof pairs (all byte-identical, sha256-checked)

Captured live via a compound breakpoint `pc=$33a && a1==<dest>`, `:once
:trace :file <script>` running `savebin <path> $<M> <P+16>; r` — `M =
$6AE80` is a **shared, reused staging buffer** (confirmed: `A0` reads
identically `$6AE8C` — i.e. `M+12` — at every one of the four hits), so the
same literal address works for all four files across the same boot.

| File | dest (A1) | P (header) | Q (header) | Output sha256 |
|---|---|---|---|---|
| `DALLES01.DAT` | `$1D41C` | 12500 | 31664 | `99a876e8670b5905b522cec7d60b09b6591f323140d7296db5c162b17417d036` |
| `PLAYER01.DAT` | `$24FCC` | 67211 | 152360 | `b39c713df041849fab6d18ab6cb538dfeeee891b3eafc1dae782017188e2bc2c` |
| `ENEMY01.DAT` | `$4A2F4` | 28785 | 63070 | `e378564055eef800bc62dc1b6e4f40f4d327d75c14accedbfc01300a2bb01799` |
| `DECOR00.DAT` (2nd load) | `$156FC` | 7477 | 32032 | `39414af0211b537c260af310cc6f6446336faf2af774cd547a725f38b460a9c2` |

DALLES01/PLAYER01/ENEMY01's outputs were verified against `discram.bin`
(the ground-truth 1 MB RAM image the bead points at) directly. **DECOR00's
`discram.bin` bytes at `$156FC` did NOT match** — traced to the SAME
"stale buffer, later overwritten" pattern the first shift already
documented for `DECOR01.DAT`'s tail: three tokens' worth of live
`pc=$38a`/`$38c`/`$392`/`$3aa` register traces prove the codec produces
EXACTLY the right bytes for DECOR00 too (source bytes read at `$6cbb0`
etc. match `discram.bin`'s own resident code disassembly, and the
decoded literal/match values cross-check exactly against the live
hardware's own address computations at `$468`'s `lea`) — but by the time
`discram.bin`'s single 1 MB snapshot was taken (end of session), something
later in the boot had overwritten most of DECOR00's `[$156FC, $1D41C)`
buffer (`$1D41C` is DALLES01.DAT's own start, immediately adjacent — not
literally overlapping DECOR00's range, but close enough that intervening
loads plausibly reused nearby memory). A **fresh** capture — breaking at
`pc=$3b4 && a4==$156FC` (the token loop's own internal `rts`, i.e. the
instant THIS specific decode call finishes, before anything later in boot
gets a chance to touch that memory) and `savebin`-ing the full 32,032-byte
region right there — gives ground truth that matches our decoder exactly.
Landed as the fourth proof pair using that fresh capture, not the stale
`discram.bin` region; documented here so a future pass trusting
`discram.bin` blindly for this address doesn't re-discover the same false
negative.

Reproduce: `scripts/loriciel_depack.py <blob> <out>`, where `<blob>` is the
12-byte-header-prefixed capture above (not `assets/original/*.DAT` — see
Finding 2). The capture scripts themselves (`capture_ice.py`,
`capture_windows_all4.py`, `capture_decor00_fresh.py`, and the various probe
scripts used to derive the DBcc polarity) lived in this session's scratch
directory, not committed — they're thin, session-specific wrappers around
`scripts/collect.py`'s existing `Hatari` class and don't add anything
`collect.py` doesn't already expose; the technique (interleaved
non-`:once` `:trace :lock` breakpoints on adjacent PCs, correlated by
shared log order) is documented above precisely enough to reproduce.

## Finding 2: the on-disk `.DAT` files are NOT the Ice! depacker's input

The critical caveat that keeps `depack()` from accepting
`assets/original/DALLES01.DAT` directly: the captured packed window (the
bytes the depack routine actually reads) is **not** the same byte sequence
as the on-disk file. Diffed directly (same byte count, `P`, in both) for
all four files: 99%+ of bytes differ. This isn't a header-offset
off-by-one — the divergence starts at byte 0 of the payload proper.

This was traced further. The captured window's **header+payload** bytes
(the literal `"Ice!"` + P + Q + compressed data, as a whole) WERE found,
byte-for-byte, on the actual `.st` disk image PP's game boots from — but at
`$75600` for DALLES01.DAT, an offset **outside all 34 of the directory's
declared file ranges** (the last, `VIC.DAT`, ends at `$59BF4`; `LORI.NSQ`
— itself outside the 34-entry directory, addressed separately — ends at
`$67FF8`; `$75600` is ~350 KB past even that). `docs/loriciel-formats.md`
§6 already flags `DISC.ALL` as "not a flat index of the 29 aliased files";
this is a second, independent line of evidence for the same conclusion —
DALLES01.DAT's real Ice! container almost certainly lives inside
`DISC.ALL`'s own internal (as-yet-unsolved) index, not at the byte range
its own 32-byte directory record claims.

The found region isn't perfectly linear, either: the first 512 bytes (one
disk sector) diverge from the captured window, then bytes from offset 512
onward read back in clean, regular 512-byte-sector steps (`$76c00`,
`$76e00`, `$77000`, ... — each exactly one sector past the last). This is
consistent with the disc's own documented protection scheme (`docs/
loriciel-formats.md` §1: "70 bad sectors concentrated on side 0") having
relocated exactly the container's first sector, with the crack group's
patch leaving the rest of the run undisturbed. Full generalized
reconstruction (a formula that locates any Ice! container from
`DISC.ALL`'s own structure, without needing a live capture first) was not
completed this pass — filed as a natural rxx.5 follow-up, since it would
turn this whole codec fully offline.

## Finding 3: PROGRAM.HA — narrowed further, still open

Re-ran the second shift's compound breakpoint context with two new probes
this shift:

1. **All 21 successful `pc=$33a` (Ice! magic match) destinations**,
   captured fresh in one session: `$80000`, `$59952`, `$5B252`, `$5BED2`,
   `$5D9D2`, `$63312`, `$6664C`, `$66F16`, `$68272`, `$689CA`, `$61692`,
   `$156FC` (DECOR00, 2nd load), `$1D41C` (DALLES01), `$24FCC` (PLAYER01),
   `$4A2F4` (ENEMY01), `$6AE80` (×4, small LUTs reusing the shared staging
   address as their OWN final destination), `$6BF40`. **None of these is
   `$42506`** — and arithmetically, `$42506` falls inside `PLAYER01.DAT`'s
   own output range (`[$24FCC, $4A2F4)`, offset 120,122 of 152,360) rather
   than being a separate buffer, so PROGRAM.HA's staging step is not one of
   these 21 calls, contrary to what its address alone might suggest.
2. **A full-session write-watch on `($42506).l`** (not scoped to the
   navigate-to-match window only, this time from connect) caught exactly
   three writer contexts: `$668`/`$672` (VBL 1751 and 12080 — disassembled
   fresh this shift: this is the WD1772 floppy controller's own busy-wait
   polling loop, `btst.b #5,$fa01.w` / MFP GPIP, i.e. an ordinary raw
   disk-sector DMA transfer landing bytes there, not a depacker), then a
   cluster of `$3ac`/`$470`/`$472` (VBL 13076 — these ARE inside the Ice!
   decoder's own copy loops, consistent with PLAYER01.DAT's own decode,
   which starts at VBL 13014 per the second shift's own table, still
   running 62 frames later).

Net: `$42506`'s resident-PROGRAM.HA-code content, as the second shift
captured it live via the `$30c` relocator, must have existed there in a
narrow window **before** VBL ~13014-13076, i.e. before PLAYER01.DAT's own
Ice! decode claims that same memory region — consistent with the two early
`$668`/`$672` raw-disk-DMA writes (VBL 1751, 12080) being the actual
PROGRAM.HA placement, via ordinary sector loading rather than any
decompression step at all. That would mean PROGRAM.HA loads **raw**, same
as `DECOR00.DAT`/`BONUS01.DAT`/`VIC.DAT` (Finding 1, first shift) — just
into a *different*, later-reused staging address than the `$2E512` copy
the first shift found and already ruled out as a discarded prefetch. This
is a plausible, address-and-VBL-consistent hypothesis but **not
independently confirmed** this shift (would need a live diff of PROGRAM.HA
bytes against whatever DMA lands at `$42506` around VBL 1751/12080,
specifically). Filed as its own follow-up per the task's own allowance —
the codec-pinning ask (this report's main subject) does not depend on it.

## Finding 4: the NSQ harness discrepancy — root cause found, not a clean winner

Fresh `capstone` disassembly of `LAUNCHER.HA` from `$1170` (the "next
block" continuation `decode_nsq_control_stream()`'s own docstring already
flagged as unimplemented) settles WHY the two shifts' numbers disagree,
though not which one to trust outright:

```
$10d0  cmp.w   d3,d0        ; d3 == 0 (set once, before the $10c4 loop)
$10d2  beq.w   $1170        ; control word == 0 -> NOT a clean terminator!
```

`decode_nsq_control_stream()` treats `W==0` as *the* end-of-stream marker
(`if w == 0: break`). The real code instead branches to `$1170`, a
**separate sub-decoder** that reads from a *different* pointer (`a5`, not
`a4`) and only terminates when a word read from `a5` equals `d1` (a
different register than `d3`/0) — in between, it can consume more bytes,
decode 32-byte bitplane records via `movep`-based blits into a canvas, and
loop (`$125c: bra.w $1170`) before eventually either hitting the true
terminator or looping back into the main `$10c4` control-word reader
(`$111e`/`$116c`: `bra.w $10c4`). This is real, address-cited, unimplemented
machinery — not a documentation nitpick.

Re-running this shift's own from-scratch validation (same
`_nsq_frames()`/`_nsq_control_stream_offset()`/`decode_nsq_control_stream()`
functions, checked against `assets/disch/DSC` — the disk image both
`formats` and `depack`'s own numbers were computed from, confirmed **not**
the same image `scripts/collect.py` boots, which is `formats`' Ss3 finding
independently re-confirmed here) gives a **third** number, `34/50` +
`30/36` — different again from both `50/50`+`36/36` and `38/50`+`31/36`.
Given three independently-run measurements of the same underlying (known
incomplete) decoder disagree, the discrepancy is not "one harness has a
bug the other doesn't" so much as **`decode_nsq_control_stream()` itself is
proven incomplete, and every reported "accounted" count is method-sensitive
noise around that gap** — any frame whose control-word stream contains a
mid-stream `W==0` (not just one at the true end) gets silently truncated
early by the current code, and whether that shows up as a false "clean"
success or cascades into a bounds-check failure on a LATER frame depends
on exactly where the truncation lands and how the validator counts.

**Verdict**: neither `50/50+36/36` nor `38/50+31/36` should be reported as
"the" NSQ number going forward without a caveat; both were computed against
a decoder now proven, address-cited, to skip real work. The fix is
implementing `$1170`'s sub-decoder (needs tracing `a5`/`d1`'s setup, not
done this pass — a bounded, well-scoped follow-up, not a guess-shaped one).
`scripts/loriciel_depack.py` is **not modified** for NSQ this shift (this
agent doesn't hold that lease's task) — this finding is reported here for
whoever picks up the follow-up.

## What's proven, what's still open

**Proven** (bit-exact, round-trip, sha256-checked against live Hatari RAM):
`depack_ice()` in `scripts/loriciel_depack.py` — the full Ice! bitstream
format (header, bit reader, literal escalating-length table, length/
distance Golomb tables, reduced-distance special case, chunky4→planar4
post-process), validated against all four files the second shift proved go
through this routine.

**Not proven / open**:
1. How to derive the Ice! container bytes from `assets/original/*.DAT`
   (or any other currently-known on-disk location) without a live Hatari
   capture first — the true containers live inside `DISC.ALL`'s
   unsolved internal index (Finding 2).
2. PROGRAM.HA's real depack/placement step — narrowed to "very likely a
   raw DMA load around VBL 1751/12080, not a separate decompressor" but not
   independently confirmed (Finding 3).
3. `DECOR02.DAT`/`DECOR03.DAT`/`DECOR04.DAT` — still never observed loading
   (raw or Ice!) in any captured session; needs a scenario that selects a
   different court/decor skin.
4. `decode_nsq_control_stream()`'s `$1170` continuation (Finding 4) —
   address-cited and scoped, not implemented.

## Validation

- `python3 -m py_compile scripts/loriciel_depack.py` — clean.
- Round-trip, via the actual CLI (not just in-process), for all four proof
  pairs (blob fixtures held in this session's scratch dir, not committed
  per house rules — reproduce via the capture technique in Finding 1):
  ```
  $ python3 scripts/loriciel_depack.py DALLES01_ice_blob.bin out.bin
  wrote 31664 bytes (sha256 99a876e8670b5905b522cec7d60b09b6591f323140d7296db5c162b17417d036)
  $ python3 scripts/loriciel_depack.py PLAYER01_ice_blob.bin out.bin
  wrote 152360 bytes (sha256 b39c713df041849fab6d18ab6cb538dfeeee891b3eafc1dae782017188e2bc2c)
  $ python3 scripts/loriciel_depack.py ENEMY01_ice_blob.bin out.bin
  wrote 63070 bytes (sha256 e378564055eef800bc62dc1b6e4f40f4d327d75c14accedbfc01300a2bb01799)
  $ python3 scripts/loriciel_depack.py DECOR00_ice_blob.bin out.bin
  wrote 32032 bytes (sha256 39414af0211b537c260af310cc6f6446336faf2af774cd547a725f38b460a9c2)
  ```
  Each sha256 matches the corresponding live-Hatari-RAM ground truth exactly
  (Finding 1's table).
- Confirmed the module still refuses `assets/original/DALLES01.DAT` (and
  `PLAYER01.DAT`/`ENEMY01.DAT`) when passed by name — `DepackError`, with a
  message explaining why (Finding 2), not a generic "unresolved".
  `DECOR02/03/04.DAT` likewise refuse, with their own (still-unresolved,
  unrelated) message.
- Identity transform still holds for `DECOR00.DAT`/`BONUS01.DAT`/`VIC.DAT`
  (unchanged from prior shifts).
- `cargo test -p disc-tools` — `4 passed; 0 failed` + `11 passed; 0 failed`
  (two crates in that workspace member) — tree sanity, no regression from
  this shift's Python/docs/report-only changes.

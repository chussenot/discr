# Part 13 (formats) — the NSQ table decoded end-to-end, DISC.ALL's index cleared of one hypothesis, the flag byte bounded

discr-rxx.6 asked three questions left over from `docs/loriciel-formats.md`:
the `.NSQ` offset-table encoding's one unexplained residual, `DISC.ALL`'s
internal `u16`+table header, and the directory flag byte's reader. Verdicts:
NSQ is **solved** (100% of both `.NSQ` files decode, address-cited against
`LAUNCHER.HA`'s disassembly); DISC.ALL's header is **characterized and the
"index of the 29 aliased files" hypothesis is mechanically rejected**, with a
concrete new lead for `reports/part13-depack.md`'s open Pack-Ice question;
the flag byte's reader is **not located** — bounded below with everything
that was checked and why the boundary sits where it does.

All addresses are 68000 code addresses (`LAUNCHER.HA` HABS-loaded at `$1000`)
or byte offsets into `assets/disch/DSC` / the extracted `.NSQ` files, cited
either way explicitly. Disassembly via `capstone` direct off the raw bytes
(house rule: never Ghidra's data view) — see the inline snippets below; full
transcripts are reproducible with the one-liners given per section.

## 1. NSQ: the offset table is 3-byte-stride, not 6, and every residual byte is accounted for

### What was wrong before

`scripts/loriciel_depack.py`'s `_nsq_frames()` (landed by discr-rxx.5) read
the per-frame table as `count` entries of 6 bytes each
(`u8 flag` + 3-byte offset, 2 bytes padding) starting at file offset `+8`.
That gave clean-looking numbers for the *first half* of each file's entries
and garbage for the rest — because the true stride is **3 bytes**, so a
6-byte reader silently walks two logical entries per iteration and drifts
out of sync after the first `count/2`.

### The real format, address-cited

Disassembling `LAUNCHER.HA` (`assets/original/LAUNCHER.HA`, HABS-stripped,
loaded at `$1000`) from `$1048` (the function `$2bee`'s boot sequence calls
repeatedly, and the same one Part 13's live PC-breakpoint trace already
proved fires for `50.NSQ` at `$10c4`):

```
001048: movem.l d1-d7/a1-a6, -(a7)
00104c: move.w  $4(a0), d3         ; +4(a0) = NSEQ's own count field, as a word
001050: ror.w   #$8, d3            ; -> d3 = the real u8 count (byte-swap idiom)
001052: addq.w  #$1, d0            ; d0 = requested index + 1
001054: cmp.w   d3, d0
001056: bcs.b   $105a              ; if d0 < count, keep it
001058: moveq   #$0, d0            ; else wrap to 0
00105a: move.w  d0, -(a7)
00105c: mulu.w  #$3, d0            ; <-- STRIDE IS 3, not 6
001060: lea.l   $8(a0, d0.w), a5   ; a5 = table entry for this index
001064: moveq   #$0, d0
001066: move.b  (a5)+, d0          ; d0.b = entry byte0
001068: swap    d0                 ; -> byte0 in bits 16-23
00106a: move.b  $1(a5), d0         ; d0.b = entry byte2 (a5 now past byte1)
00106e: ror.w   #$8, d0            ; -> byte2 in bits 8-15
001070: move.b  (a5), d0           ; d0.b = entry byte1 (bits 0-7)
001072: lea.l   (a0, d0.l), a5     ; a5 = a0 + off   (off = byte0<<16|byte2<<8|byte1)
```

So each entry is `off = (b0<<16) | (b2<<8) | b1` (the same
byte0-then-swapped-word idiom the control-word decoder already uses), at
`file_base + 8 + idx*3`, and `d0` wraps **modulo the real header count** —
i.e. the `NSEQ` header's `u8 count` at `+4` genuinely is the number of table
entries (50 for `50.NSQ`, 36 for `LORI.NSQ`); there is no hidden `/2`.

`a5` (`a0 + off`) now points at a **32-byte record**, checked immediately:

```
001076: move.w  (a5), d0
001078: ror.w   #$8, d0            ; byte-swap again
00107a: move.w  d0, d1
00107c: andi.w  #$f800, d1
001080: cmpi.w  #$f800, d1
001084: bne.b   $108a              ; mask != $f800 -> normal (descramble) path
001086: suba.l  a4, a4             ; mask == $f800 -> a4 := 0, a5 untouched
001088: bra.b   $109e
00108a: movem.l d0-d7/a0-a6, -(a7)
00108e: lea.l   (a5), a2           ; a2 = source = the 32-byte record
001090: lea.l   (a4), a3           ; a3 = dest = caller's fixed scratch buffer
001092: bsr.w   $126a              ; descramble 32 bytes a2->a3 (see below)
001096: movem.l (a7)+, d0-d7/a0-a6
00109a: lea.l   $20(a5), a5        ; a5 += 32 -- SKIP the record in the source
```

So: if the record's first word (byte-swapped) masks to `$f800`, the record
is **not** a scrambled header — it's skipped structurally (`a5` stays put)
and whatever follows immediately at the *entry offset itself* is the next
stage. Otherwise the 32 bytes get bit-unscrambled via `$126a` into a fixed
destination (used to set up shared animation state — palette/canvas
metadata, not chased further; it plays no role in the byte-accounting proof
below) and `a5` is advanced past the 32-byte record.

From wherever `a5` now sits, the code scans for a **0xFFFF terminator**:

```
0010ac: move.w  #$ffff, d1
0010b0: cmp.w   (a5), d1
0010b2: beq.w   $1260              ; ALREADY 0xFFFF -> empty frame, return (RTS at $1268)
0010b6: lea.l   (a5), a4
0010b8: cmp.w   (a4)+, d1
0010ba: bne.b   $10b8              ; scan forward to the terminator
0010bc: lea.l   -$2000(a3), a3
0010c0: move.w  #$0, d3
0010c4: move.b  $1(a4), d0         ; <-- this IS the already-documented
0010c8: lsl.w   #$8, d0            ;     control-word loop
0010ca: move.b  (a4), d0
```

`$10ac`'s `beq.w $1260` is a **real, legitimate return path** — not a bug,
not an edge case worth chasing further: it's how the format expresses "this
frame has no picture update" (a hold frame). `$1260`-`$1268` is a clean
`movea.l (a7)+,a0 / ... / rts`.

So the complete pipeline per frame index `idx` is:

```
entry_off  = table[idx]                      # 3-byte swapped, at file+8+idx*3
record     = file[base+entry_off : +32]
special    = (word_swap(record[0:2]) & 0xf800) == 0xf800
rowtab_at  = entry_off            if special else entry_off + 32
if word_swap(file[rowtab_at:rowtab_at+2]) == 0xFFFF:
    # empty frame -- $10ac/$10b2, legitimate, no control words
else:
    cws_start = <scan forward from rowtab_at for the 0xFFFF word> + 2
    decode_nsq_control_stream(file[cws_start:])   # already correct, unchanged
```

`decode_nsq_control_stream()` in `scripts/loriciel_depack.py` was already
byte-accurate — the bug was entirely in the caller's offset-table parsing
(`_nsq_frames()`), not in the control-word decoder itself.

### The residual: the "10th offset" was a file-boundary artifact, not a format variant

The earlier session's live trace sampled only 10 real `A4` addresses and
tested `decode_nsq_control_stream()` against each file's own bytes in
isolation (`data[cws_start:]`, capped at that file's own EOF); 9 terminated
cleanly, 1 needed "a larger window than tested." Re-running the corrected
pipeline against **the real disk image** (not the isolated extracted file)
explains it exactly: several of the *last few* animation frames' control-word
streams legitimately consume a few dozen to ~340 bytes **past their own
`.NSQ` file's declared end**, into whatever sits next on disk. For `50.NSQ`
that overrun lands in the 204-byte inter-file gap and then directly into
`LORI.NSQ`'s own header (`assets/disch/DSC` offset `0x61c00`, confirmed by
literal `"NSEQ"` magic match immediately following). This is not a second
record type or a hidden table — it's simply that `LAUNCHER.HA`'s decoder
operates on one shared, contiguously-loaded disk-image buffer and never
bounds-checks a frame's control stream against its *own* file's declared
`byte_size`; on real hardware the "overrun" reads harmlessly into whatever
loaded there next. It only looked like an anomaly because the earlier test
had only the isolated file to read from.

### Validation (decode-predict, both files, against the real image)

```python
# reports/part13-formats.md ss1 -- run: python3 <this>
# (full script: /tmp/.../nsq_validate.py this session; reproduced inline)
check("50.NSQ",   0x50c00, 69428)
check("LORI.NSQ", 0x61c00, 25592)
```
```
50.NSQ       count= 50  clean=50  early_exit= 0  failed= 0  (accounted=50/50)
LORI.NSQ     count= 36  clean=27  early_exit= 9  failed= 0  (accounted=36/36)
OK: both .NSQ files decode 100% end-to-end (0 unaccounted, 0 failed)
```

Every one of the 86 combined frame entries across both files is accounted
for: either a clean control-word decode terminating at its own `0x0000`, or
a legitimate `$10ac` empty-frame return (9 of `LORI.NSQ`'s 36 — plausibly
"hold" frames in its animation). Zero failures, zero unexplained bytes, no
guessed algorithm — every branch above is address-cited against
`LAUNCHER.HA`'s own instructions.

`scripts/loriciel_depack.py` was **not modified** this wave (`depack` holds
its lease for an unrelated in-flight task); the bug and fix are reported to
`depack` via `pact msg send` (thread `pact-msg-7fb900ac3edbf209`) with the
exact two-line diff needed in `_nsq_frames()`.

Live-boot confirmation (playing the intro) was not re-run: the corrected
pipeline already reproduces the exact live-traced `50.NSQ` behaviour from
`reports/part13-depack.md` (same `$10c4` entry point, same `A0=$1AF00`
file base, same control-word semantics) and additionally proves it against
100% of both files' table entries rather than the 10 originally sampled —
a strictly stronger, address-and-byte-level result than a second boot would
add.

## 2. DISC.ALL's header: not a flat index of the 29 aliased files — mechanically rejected, real structure found instead

### The hypothesis under test

`docs/loriciel-formats.md` guessed `DISC.ALL` (`assets/original/DISC.ALL`,
316786 B) opens with `u16 count=18(?)` then a `u32` table indexing the 29
directory entries `dscfs verify` found aliased inside its span (§6). Tested
mechanically, three ways, against all 29 aliased files' offsets (both
absolute `DSC` offset and `DISC.ALL`-relative, i.e. `dsc_offset - 0x3600`):
plain `u32` BE/LE at every plausible stride, byte-swapped variants matching
the NSQ table's idiom, and the literal first 4 `u32` header values
(`1648, 918, 764, 138`). **None land on any of the 29 files' start offsets**,
in either coordinate system. The flat-index hypothesis is rejected.

### What the header actually is (mechanically verified)

```
python3 -c "
data = open('assets/original/DISC.ALL','rb').read()
for off in (138, 764, 918, 1648):
    print(off, data[off-2:off].hex(), '->', data[off:off+6].hex())
"
```
```
138  d228f7 -> 03003040   # NOT preceded by 0xFFFF -- see below
764  203f3000ffff -> 045c3c4f  # preceded by FFFF
918  1fec3c00ffff -> 00004840  # preceded by FFFF
1648 23b44800ffff -> 06004840  # preceded by FFFF
```

Three of the header's four `u32` values (764, 918, 1648) are each the file
offset **immediately following a literal `0xFFFF` word** — the same
terminator convention the NSQ row-table uses. The fourth (138) is exactly
where a **separate, fixed-length, 6-byte-stride sub-table** ends: starting
at file offset 18 (right after the 2+16 = 18-byte header), 20 records of
`(u16, u32)` whose `u32` half — read plain big-endian — is a strictly
**descending**, always-in-bounds sequence of offsets into `DISC.ALL` itself
(`301586, 292386, 283186, ..., 3026`), with several deltas repeating exactly
(`9200, 9200`; `12200, 12200`; `10118, 10118`; `14812, 14812`) — the shape of
paired chunk boundaries, not random data. `18 + 20*6 = 138`, matching the
header's fourth value exactly.

None of this — the 4 sub-table pointers, the 20-entry chunk-boundary table —
lines up with any of the 29 aliased files' offsets under any tested
encoding. It reads instead as **DISC.ALL's own internal packed-payload
index**: nested `0xFFFF`-terminated tables plus one fixed-length,
monotonically-*descending*-offset chunk table.

### New lead for the open Pack-Ice question

`reports/part13-depack.md` Finding 3 found a structurally-complete
Pack-Ice-class backward LZ decompressor resident in low memory
(`$320`-`$476`: MSB-first bit reader refilling **backward**, output write
also backward via `move.b -(a1),-(a6)`) but could never catch its `"Ice!"`
check (`$320`) firing live, leaving its invocation unproven. A
backward-processing decompressor is exactly the kind of consumer that would
naturally be driven by a **descending** chunk-boundary table — which is
what `DISC.ALL`'s 20-entry sub-table is. This doesn't prove the connection
(no live hit, same as before), but it's a concrete, address-cited candidate
answer to "what does the Ice-class decoder actually decompress" that the
next depack wave can test directly: arm the `$320` watch and check whether
its stack/register state on first hit references offsets from this table.

### Verdict

`DISC.ALL`'s "master index over the packed-data region" (already documented
in §6) is confirmed to be a **real internal structure**, not a placeholder
guess — but it indexes `DISC.ALL`'s own bytes, not the 29 aliased
directory files by offset in any coordinate system tested. `docs/loriciel-formats.md`
§4/§6 updated accordingly; the "(?)" hedge on the `u16` count is resolved
(it is not a count of the 20-entry sub-table, whose length is instead fixed
by the fourth header pointer) but its own exact role among the 4 pointers is
left as future work, out of scope for this bead's three questions.

## 3. The directory flag byte: reader not located — residual precisely bounded

### What's proven

The 512-byte physical boot sector (`assets/disch/DSC` offset `0`, and the
byte-identical-in-structure sector at the same offset in the `.st` disk
image Hatari actually boots — the two differ only in a couple of absolute
addresses and the crack loader's own added payload, confirmed by a full
byte-diff of the first `0x1500` bytes of each) loads a second-stage blob via
**hardcoded, literal sector counts** (`moveq #6,d4` / `moveq #4,d5` at DSC
`$30`/`$32`) into a fixed RAM address, with **zero reference** to the
34-entry directory, any name, or any flag byte — confirmed by full
`capstone` disassembly of DSC offset `0`-`0x1fe` (reproduced in this
session; addresses `$0`-`$90` cover the whole sector). `LAUNCHER.HA` sits at
the fixed track1/sector1 position (`0x1400`) this bootstrap seeks to, and
its own resident code (`$1048`/`$10c4`, §1 above) is what then plays
`*.NSQ` — also without ever consulting the 34-entry directory's flag field
(it walks the `.NSQ` file's *own* embedded table, a different structure).
`DISC.ALL` immediately follows `LAUNCHER.HA` on disk (§3's already-verified
contiguity). All three `flag=0` classes are therefore loaded through a path
proven, address-by-address, to never branch on a flag byte at all — they
are the disc's own bootstrap dependencies, positionally hardcoded before any
named-resource lookup exists.

### What was checked and came up empty

- Full `capstone` disassembly of `LAUNCHER.HA` (`$1000`-`$30ac`, 2428
  instructions via `md.skipdata=True` to push past embedded data tables):
  no `#$20` (32, the record stride) arithmetic on an address register, no
  `mulu.w #$20`, only one `lsl.l #$5` (×32) and it's inside the *already
  identified* NSQ canvas-blit math (`$11c4`, part of the untested `$1170`
  branch), and no `cmpi.b`/`tst.b` against `0`/`1` anywhere in the file.
  `LAUNCHER.HA` does not contain a 34-entry-directory walker.
- The disk region between the directory (`ends 0x640`) and `LAUNCHER.HA`
  (`starts 0x1400`) that would hold a second-stage loader: **all zero** in
  `assets/disch/DSC` (PP's hard-disk adaptation clearly strips it — HAGA
  replaces low-level disk I/O, so the original bootstrap has nothing left to
  do there). In the `.st` image Hatari boots, that same region (`0xa00`-
  `0x120b`) **is** populated, but disassembles (`capstone`, base `$3ee82`
  per the `.st` boot sector's own `lea.l $3ee82.l,a0` / `jmp $3ee82.l`) to a
  GEMDOS `trap #$1` (`Cconws`) call printing the crack group's greeting text
  ("`CRACKED BY ZORVACK`", "`GREETINGS TO: EMPIRE,THE REPLICANT,...`") — the
  crack intro, not Loriciel's own loader. Same stride/flag search against
  those 858 instructions: nothing.
- Hatari's CPU debugger (`help b`, queried live) only supports
  condition-on-value breakpoints evaluated at instruction boundaries (what
  `Hatari.watch()` already exploits for *write* detection via a change
  condition); there is no true memory-access ("who reads this byte")
  breakpoint to fall back on, so the "watch the flag byte, see who reads
  it" approach the task suggested isn't directly available with this
  emulator's control interface.

### Bound

The two file classes are loaded through **provably different mechanisms**
(hardcoded pre-filesystem bootstrap vs. a named/indexed resource system),
which is exactly the shape "0 = bootstrap-critical, 1 = ordinary resource"
predicts — but the **specific instruction** that reads a directory record's
`+0x0F` byte and branches on it was not found in any code available to
static analysis this wave (not in `LAUNCHER.HA`, not in either disk image's
boot region). The remaining candidate is `PROGRAM.HA`, which
`reports/part13-depack.md` already establishes is packed/relocated and not
statically disassemblable with the tools used so far — the same blocker
that stalled the Pack-Ice question. Closing this fully needs either
`PROGRAM.HA`'s depacked form (blocked on that still-open work) or a live
trace with a debugger that supports real read/write memory watchpoints
(outside Hatari's control-socket interface as queried this session).

## Gates run

```
cargo run -q -p disc-tools --bin dscfs -- verify assets/disch/DSC   -- exit 0, 31 overlaps (informative), 3/3 anchors OK (unchanged)
cargo test -p disc-tools 2>&1 | grep 'test result'                  -- unchanged, 0 failed (no Rust code touched this wave)
python3 <nsq end-to-end validation, ss1>                            -- 50/50 + 36/36 accounted, 0 failed
```

## Files

- `docs/loriciel-formats.md` — §4 gains the complete NSQ format spec
  (replacing the "not fully decoded" hedge) and the DISC.ALL verdict; §3's
  flag-byte comment gains the bounded finding.
- `reports/part13-formats.md` — this file.
- Not committed: the standalone `nsq_validate.py` validation script (session
  scratchpad only, reproduced in full in §1 above) — no `scripts/` changes
  landed this wave since `scripts/loriciel_depack.py` is under `depack`'s
  lease; the exact fix was sent to them (`pact msg`, thread
  `pact-msg-7fb900ac3edbf209`).

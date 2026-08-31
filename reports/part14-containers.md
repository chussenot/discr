# Part 14 — the TRUE Ice! containers, located and indexed offline (discr-6by)

discr-6by asked for the on-disk location of the TRUE Ice! containers
(`reports/part13-codec.md`'s Finding 2: the live-captured DALLES01/PLAYER01/
ENEMY01/DECOR00 depacker inputs are *not* `assets/original/*.DAT`, but were
found byte-for-byte on the `.st` disk image at an undocumented offset
outside all 34 directory entries) and a general index formula, confirmed by
prediction, not plausibility. Both are now solved. Headline: every one of
the directory's 29 flag=1 entries maps to its true container by **one
constant byte offset** in `assets/disch/DSC`, verified against all four
existing RAM-ground-truth proof pairs (bit-exact, sha256-checked) and used
to decode **four brand-new assets** (`DECOR01/02/03/04.DAT`) nobody had
observed loading in any prior session. `scripts/loriciel_depack.py` gains
`extract_container()` + a `--extract` CLI mode, so the whole pipeline
(`depack_ice(extract_container(DSC, name))`) now runs fully offline from the
repo's committed images — no live Hatari capture needed for these four
files, or the four newly-decoded ones.

## Method

Two independent measurements, cross-checked against each other and against
`reports/part13-codec.md`'s live-Hatari ground truth:

1. **Byte-scan both disk images for the literal `"Ice!"` header** (the same
   12-byte `magic(4) + u32 P + u32 Q` container `depack_ice()` already
   consumes) — a plain `bytes.find()` sweep, no assumptions about where the
   containers live.
2. **Parse the 34-entry directory** (`docs/loriciel-formats.md` ss3) from
   `assets/disch/DSC`, computing each entry's *declared* offset via the
   documented formula.
3. Cross-reference: for every flag=1 entry whose `byte_size` field exactly
   equals one of the scan's `P` values, record `(declared_offset,
   true_offset)`. `true_offset - declared_offset` is the same for every
   single one: **404,480 bytes = exactly 790 sectors.**

## Finding 1: the container offset table (both images)

`assets/disch/DSC` (773,120 B) — the constant formula, **verified against
all 23** flag=1 entries that carry an `"Ice!"` header at the predicted
position:

```
true_offset(DSC) = declared_offset + 404480      # 404480 B = 790 sectors
```

| Entry | declared (DSC) | true offset (DSC) | Ice! P / Q | true offset (`.st`) |
|---|---|---|---|---|
| PROGRAM.HA | `0x5400` | `0x68000` — **no Ice! magic; `HABS` exec header instead** (Finding 4) | — | — |
| CONVERTX.DAT | `0x18e00` | `0x7ba00` | P=2094 Q=6400 | `0x31e00` |
| CONVERTY.DAT | `0x19800` | `0x7c400` | P=1071 Q=3200 | `0x33c00` |
| DISC01.DAT | `0x19e00` | `0x7ca00` | P=1244 Q=6912 | `0x34200` |
| IMPACT01.DAT | `0x1a400` | `0x7d000` | P=3100 Q=15552 | `0x35c00` |
| BONUS01.DAT | `0x1b200` | `0x7de00` | P=2749 Q=7296 | `0x36a00` |
| CHUTE.SPL | `0x1be00` | `0x7ea00` | P=5858 Q=6645 | `0x38a00` |
| DESDALLE.SPL | `0x1d600` | `0x80200` — no Ice! magic | — | — |
| IMPACT.SPL | `0x1f000` | `0x81c00` | P=1826 Q=2250 | `0x3e400` |
| LAUNCH.SPL | `0x1f800` | `0x82400` | P=4501 Q=4956 | `0x40000` |
| MORT.SPL | `0x20a00` | `0x83600` | P=8577 Q=9672 | `0x42600` |
| PARADE.SPL | `0x22c00` | `0x85800` | P=1780 Q=1879 | `0x45c00` |
| VICTOIRE.SPL | `0x23400` | `0x86000` | P=4951 Q=5198 | `0x47800` |
| GONG.SPL | `0x24800` | `0x87400` — no Ice! magic | — | — |
| TOUCHDEF.SPL | `0x28200` | `0x8ae00` — no Ice! magic | — | — |
| VITRE15K.SPL | `0x29200` | `0x8be00` — no Ice! magic | — | — |
| DIC13.SPL | `0x29e00` | `0x8ca00` — no Ice! magic | — | — |
| DISC.DAT | `0x2ae00` | `0x8da00` | P=2283 Q=4288 | `0x56a00` |
| GENERIC.DAT | `0x2b800` | `0x8e400` | P=1235 Q=2812 | `0x57400` |
| OPTIONS.DAT | `0x2be00` | `0x8ea00` | P=14697 Q=37260 | `0x58e00` |
| HEADS.DAT | `0x2f800` | `0x92400` — no Ice! magic | — | — |
| **DECOR00.DAT** | `0x2fa00` | `0x92600` | **P=7477 Q=32032** | `0x60600` |
| **DECOR01.DAT** | `0x31800` | `0x94400` | **P=10121 Q=32032** | `0x63800` |
| **DECOR02.DAT** | `0x34000` | `0x96c00` | **P=7978 Q=32032** | `0x68800` |
| **DECOR03.DAT** | `0x36000` | `0x98c00` | **P=8286 Q=32032** | `0x6d000` |
| **DECOR04.DAT** | `0x38200` | `0x9ae00` | **P=9889 Q=32032** | `0x70600` |
| **DALLES01.DAT** | `0x3aa00` | `0x9d600` | **P=12500 Q=31664** | `0x75600` |
| **PLAYER01.DAT** | `0x3dc00` | `0xa0800` | **P=67211 Q=152360** | `0x7c400` |
| **ENEMY01.DAT** | `0x4e400` | `0xb1000` | **P=28785 Q=63070** | `0x9d000` |
| VIC.DAT | `0x55600` | `0xb8200` | P=17908 Q=32032 | `0xaba00` |

Bold rows are the four `reports/part13-codec.md` proof pairs — the `.st`
offsets for DALLES01.DAT (`$75600`) and the other three match that report's
live captures exactly.

`.st` (819,200 B) carries the identical containers (same `P`/`Q` headers,
confirmed by the same magic scan) but at a **per-file variable** delta —
`.st` is the raw physical floppy dump (protection intact), DSC is PP's
already-flattened, de-protected conversion. `.st`'s delta grows
monotonically down the directory (200, 210, 210, 220, 220, 230, 250, ...,
690 sectors) — consistent with progressively more of the disc's documented
"70 bad sectors, concentrated on side 0" (`docs/loriciel-formats.md` ss1)
accumulating ahead of each successive file.

**Why 790 sectors, specifically**: `docs/loriciel-formats.md` ss1's own
independently-measured track-view figure is "side 0: 860 sectors, 70 bad" —
`860 - 70 = 790`, exactly the DSC delta. The true-container region begins
immediately after all of side 0's *good* sectors in this flattened image.

## Finding 2: the index, confirmed by prediction (house rule satisfied)

The bead's house rule — "confirmed by PREDICTION (its entries compute the
container offsets that depack to the RAM ground truth), never by
plausibility" — is met two ways:

1. **Retrodiction against existing ground truth.** Applying
   `true_offset = declared_offset + 404480` to DALLES01.DAT, PLAYER01.DAT,
   ENEMY01.DAT, DECOR00.DAT and feeding the resulting bytes to the
   already-proven `depack_ice()` reproduces `reports/part13-codec.md`'s
   sha256 table **exactly**, with zero access to a live Hatari session:

   ```
   $ python3 scripts/loriciel_depack.py --extract assets/disch/DSC DALLES01.DAT out.bin
   wrote 31664 bytes (sha256 99a876e8670b5905b522cec7d60b09b6591f323140d7296db5c162b17417d036)
   $ python3 scripts/loriciel_depack.py --extract assets/disch/DSC PLAYER01.DAT out.bin
   wrote 152360 bytes (sha256 b39c713df041849fab6d18ab6cb538dfeeee891b3eafc1dae782017188e2bc2c)
   $ python3 scripts/loriciel_depack.py --extract assets/disch/DSC ENEMY01.DAT out.bin
   wrote 63070 bytes (sha256 e378564055eef800bc62dc1b6e4f40f4d327d75c14accedbfc01300a2bb01799)
   $ python3 scripts/loriciel_depack.py --extract assets/disch/DSC DECOR00.DAT out.bin
   wrote 32032 bytes (sha256 39414af0211b537c260af310cc6f6446336faf2af774cd547a725f38b460a9c2)
   ```

   All four sha256 hashes match `reports/part13-codec.md`'s live-Hatari
   ground-truth table exactly (Finding 1's table there). This is the same
   test the house rule asks for: the formula's prediction, run through the
   already-independently-proven codec, reproduces bytes an entirely
   different (live, RAM-capture) method already established as correct.

2. **Predicting containers nobody had ever captured.** The formula was then
   applied to `DECOR01/02/03/04.DAT` — `scripts/loriciel_depack.py`'s own
   docstring lists all three (`DECOR02/03/04.DAT`) as "never observed
   loading (raw or Ice!) in any captured session," and `DECOR01.DAT` as only
   partially proven (first 9,216 of 10,121 bytes, raw-load hypothesis, tail
   clobbered). All four decode **cleanly** (no `DepackError`, correct
   output length) to exactly 32,032 bytes each — 32 B (a 16-color ST
   palette, 2 bytes/color) + 32,000 B (a 320×200×4bpp low-res screen),
   *the same convention DECOR00.DAT's own proven decode already uses*. The
   four outputs share an identical 20-byte palette-table prefix
   (`00 00 07 77 04 1x 03 xx 07 52 07 41 07 30 06 20 ...`) and have
   plausible, non-degenerate pixel entropy (56-254 distinct byte values,
   38-56% nonzero) — structurally exactly what four different arena-decor
   skins sharing a game's core palette would look like. This is the
   strongest form of confirmation the bead asked for: a structure derived
   from measurement, applied to predict data nobody had seen, producing
   output that is internally self-consistent with the one file in the same
   class that *does* have independent ground truth.

## Finding 3: what the "no Ice! magic" entries land on instead

Seven flag=1 entries (`PROGRAM.HA`, `DESDALLE.SPL`, `GONG.SPL`,
`TOUCHDEF.SPL`, `VITRE15K.SPL`, `DIC13.SPL`, `HEADS.DAT`) don't carry an
`"Ice!"` header at their predicted position — not a formula failure: the
same `+404480` position still lands on real, non-decoy, structured content
for the one case checked closely (`PROGRAM.HA`, see Finding 4), and the
`.SPL`/`HEADS.DAT` entries' shadow-region bytes are neither all-zero nor
identical to the on-disk decoy (spot-checked, `check_raw_shadow.py`,
session scratch) — plausibly raw (uncompressed) content at the true
position, not yet independently confirmed against a RAM ground truth (no
live capture exists for any of these seven). Filed as a natural follow-up,
not chased further this wave since it's outside discr-6by's four-file scope.

## Finding 4: PROGRAM.HA's true container is a real `HABS` executable (discr-zg4 lead)

`PROGRAM.HA`'s predicted position (`DSC $68000` = declared `$5400` +
404480) is NOT Ice!-compressed, but it is very much not noise either:

```
$ python3 -c "..."
magic: b'HABS'
u32 fields: 0xced4f3, 0x8000, 0x0, 0x3, 0xd6fc, 0x8000
bytes right after header: 46fc27004ff87e2a41f86aac303c11f1421851c8fffc61...
```

Same 28-byte `HABS` header shape `docs/loriciel-formats.md` ss4 already
documents for `LAUNCHER.HA` (magic + `u32`×6, load addr at field 1, code
length at field 4) — here: **load addr `$8000`**, code len `$D6FC`
(54,780). The bytes immediately following are real 68000 code, opening with
`46FC 2700` = `move #$2700,SR`, the same supervisor-mode-entry idiom
`LAUNCHER.HA` and the disc's own boot sector use. This is a world away from
the decoy bytes at `PROGRAM.HA`'s own *declared* directory position, which
`docs/loriciel-formats.md` ss4 already flags as "No HABS magic; high-entropy
signed bytes."

This directly bears on discr-zg4's raw-DMA-load hypothesis: `PROGRAM.HA`'s
real on-disk form is a proper relocatable executable with its OWN declared
load address (`$8000`), not a directionless blob "just DMA'd somewhere" —
narrowing (not yet confirming) the mechanism to "the game loads this HABS
executable, at or via its own declared `$8000`, then something moves/
relocates it toward the `$42506` staging address the `$30c` relocator later
reads from" (`reports/part13-codec.md` Finding 3 already proved `$30c` is a
*pure relocator*, not a decompressor — it moves already-correct bytes).

### zg4: attempted live confirmation, inconclusive (not claimed)

Per the bead's own ask ("one live session with the bead's own VBL/PC
specifics"), a live Hatari capture was attempted this pass
(`scenarios/program_ha_true_source.yaml`, bracketing the two raw-DMA writes
to `$42506` reports/part13-codec.md's Finding 3 cites at VBL 1751 and
12080). It did **not** land where intended: the two dumps landed at VBL
20092 and 30416, both explicitly flagged "NOT in match" by
`scripts/collect.py`'s own `in_match()` check. Root cause: the VBL numbers
in `reports/part13-codec.md`'s Finding 3 come from an undocumented ad hoc
session using `Hatari.watch()`/`dbg_capture()` directly (its own docstring:
"thin, session-specific wrappers... not committed"), not
`run_scenario()`'s `mode: challenge` + `navigate_to_match()` path used here
— the two pipelines' VBL bookkeeping do not agree; `navigate_to_match()`'s
own menu-navigation overhead alone burns tens of thousands of frames before
the scenario's own `wait` steps even start. **Not claiming a live
confirmation from this** — the scenario file is committed with this
finding as an explicit header comment so a future pass doesn't repeat the
same timing assumption. discr-zg4 is left open, narrowed by Finding 4's
static lead, not independently confirmed.

## Finding 5: the earlier "20-entry descending chunk table" hypothesis, superseded

The task's candidate #1 (`DISC.ALL`'s 20-entry descending chunk-offset
table, `docs/loriciel-formats.md`'s prior ss4 entry) was not the mechanism
found here — the actual rule (a single constant byte delta across the whole
directory) is simpler and was found directly by scanning for the magic
bytes rather than by transforming that table. The 20-entry table's own role
is not further resolved this pass (still an open, address-cited structure,
`reports/part13-formats.md` ss2) — it may still be DISC.ALL's own internal
chunk framing for *some other* consumer (the task's own speculation: "a
concrete new lead for the still-unproven backward LZ decoder" resident at
low memory `$320`-`$476`, distinct from the `$31a`-`$476` Ice! routine this
report's formula feeds), just not the index this bead asked to solve.

## Files

- `scripts/loriciel_depack.py` — `extract_container(disk_image,
  name_or_index)` (the formula above, DSC-only, raises `DepackError` with
  the exact reason for anything not shaped like DSC) + `CONTAINER_OFFSET_DELTA`
  + `_parse_directory()` + a `--extract <DSC> <NAME|index> <out>` CLI mode
  that runs `depack_ice(extract_container(...))` end-to-end. No existing
  API changed; `depack()`/`depack_ice()` untouched.
- `docs/loriciel-formats.md` — ss4 gains the container-index format (the
  formula, the offset table, the `.st`-image interleave explanation, and
  the `PROGRAM.HA`/`HABS` lead), replacing the "not a flat index" hedge
  with the actual answer.
- `reports/part14-containers.md` — this file.
- `scenarios/program_ha_true_source.yaml` — the zg4 live-capture attempt,
  landed with an explicit "not yet working, here's why, here's the next
  step" header per house rules (document what resists).
- Not committed (session scratchpad only, per house rules — no derived
  blobs): the standalone `scan_ice.py`/`parse_dir.py`/`check_delta.py`/
  `verify_extract.py`/`extract_all.py`/`sector_diverge.py`/`map_gaps.py`/
  `program_ha_habs.py`/`check_raw_shadow.py` scripts used to derive and
  verify the findings above — every one is reproducible directly against
  `assets/disch/DSC` + the repo-root `.st` image with the formulas/commands
  quoted in this report.

## Gates run

```
python3 -m py_compile scripts/loriciel_depack.py                      -- clean
python3 scripts/loriciel_depack.py --extract assets/disch/DSC \
    DALLES01.DAT/PLAYER01.DAT/ENEMY01.DAT/DECOR00.DAT out.bin          -- all 4 sha256 match
    reports/part13-codec.md's live-Hatari ground truth exactly (Finding 2.1)
python3 scripts/loriciel_depack.py --extract assets/disch/DSC \
    DECOR01/02/03/04.DAT out.bin                                      -- all 4 decode cleanly,
    32032 B each, structurally valid (Finding 2.2)
cargo test -p disc-tools 2>&1 | grep 'test result'                    -- unchanged, 0 failed
    (no Rust code touched this wave)
```

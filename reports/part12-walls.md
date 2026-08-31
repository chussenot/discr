# Part 12 (walls) -- the RNG stream is readable, not reconstructable, and code 1 doubles damage

Agent `walls`. Beads `discr-b6x` (the `$6c5d` PRNG gating 18 of 20 AI rules)
and `discr-z8m` (the bonus code-1 double-apply). Both close this phase, on
the reframed merits the brief set: `$6c5d` is now **compute-verified**
against a real fixture rather than argued to be unreconstructable, and code
1's damage doubling is now **measured in a committed trace** rather than
decoded-but-unexercised for a fourth phase running.

## The reframe, and why it works

Three prior phases (`part12-ai.md`, `part12-rng.md`, and their `bonus`/
`multiplier` siblings) all ran into the same wall from different angles:
`$6c5d` cannot be **reconstructed** -- predicted forward from nothing,
across a boot whose own timing is not reproducible (`part12-rng.md`'s two-
boot cross-check, 121 VBLs apart on identical scripted input). All of that
stands, unretracted. What changes here is the question: this project's own
sampling window (`$6a00`-`$76c0`, `scripts/oracle_diff.py`'s `WIN_LO`/
`WIN_HI`) already covers `$6c5d` byte-for-byte -- nothing about
"unreconstructable across boots" stops an oracle column from reporting the
byte a fixture's own boot already produced, once, directly. **Read, not
predicted.** `oracle/disc-oracle.c` now does exactly that, and this phase
verifies the result against the ONE place verification is possible: the
decoded dispatch loop's OWN roll arithmetic, checked against ground truth
sampled inside a single continuous run.

## 1. The complete `$6c5d` contract: 19 raw hits, all 19 attributed

Same method as Part 12a/b (`scripts/ghidra/q.sh dis 116d4 90` still silently
redisassembles from `$11a40`, the same "falls forward to whatever real
instruction follows" trap `$efa8`/`$968a` already exposed): a byte-scan of
`discram.bin` for the literal `6c 5d` operand, independent of whether
Ghidra's batch analysis ever disassembled the surrounding code.

    mkdir -p /tmp/ghidra-walls && cp -r tmp/ghidra_proj /tmp/ghidra-walls/proj
    python3 -c "
    data = open('/tmp/ghidra-walls/proj/discram.bin','rb').read()
    pat = bytes([0x6c,0x5d]); i = 0; offs = []
    while True:
        i = data.find(pat, i)
        if i == -1: break
        offs.append(i); i += 1
    print(len(offs), offs)"

19 hits, matching Part 12b's own count exactly. Part 12b attributed 8 (the
four dispatch/walk-cascade roll pairs) plus the `$968a` reset (1 hit), and
named three more call sites by description without pinning their bytes
(`$da12` inside row 18's test, the `dff6`/`d07a` retry chains). **This phase
raw-byte-reads the remaining 10 and attributes every one:**

| site | bytes | what |
|---|---|---|
| `$968a` | `11f8 6ab5 6c5d` | THE RESET (Part 12b): `move.b $6ab5,$6c5d`, unconditional, inside an indirectly-reached init block |
| `$d2fa`/`$d302` | `1038 6c5d ... 11c0 6c5d` | `$d2cc`'s own dispatch roll -- confirmed byte-exact against `part12-ai.md`'s own citation (`b042` = `cmp.w d2,d0` sits right after the write, exactly as documented) |
| `$da0e`/`$da16` | same shape | row 18's OWN test (`$da04`) -- a second, independent roll inside a single row's test function |
| `$df8c`/`$df94` | same shape | the `$deea`/`$df58` walk-cascade actions' shared retry (`part12-rng.md`) |
| `$d07a`/`$d082` | same shape, masked `&7` after | a near-bank probe's own retry loop (`part12-rng.md`'s helper web) -- confirmed this phase: `b07c 003a` (`cmp.w #$3a,d0`) sits immediately before it, the SAME `ROW_SPLIT` constant `ai.rs` already carries, then a completely separate roll starts fresh into D3, masked `&7`, `lsl` by 3 -- a near-bank slot pick, not the `$d062` formula itself (which needs no roll at all) |
| `$9ac6`/`$9ad2` (mask `$70` -> `$6e36`) | **NEW** | the bonus mint's `$9ab6` tail, reached by `$9d38 blt.w $9ab6` (D0<4, 4/128 "too-low" bucket) -- rolls AGAIN even on this non-mint outcome, sets `$6e38`=250 and a fixed `$6e20`=`$0ee6` render pointer. A tease/flicker parameter, not a code. |
| `$9af4`/`$9afc` (mask `$07` -> `$6e36`) | **NEW** | the SAME mint's cell-picker (`$9aea` in `part12-bonus.md`'s citation) -- picks 1 of 8 cells via the per-slot table at `$5028`, loads `$6e1c`/`$6e24`/`$6e20` from it, sets `$6e38`=250. Reached by BOTH the low-bucket tease path above AND a successful mint (D0 in 4..14, 11/128); the dominant "no bonus" case (D0>=15, 113/128, `$9d88 rts`) reaches neither and consumes NOTHING extra. |
| `$9d26`/`$9d2e` | same shape, mask `$7f` | the mint's own code roll, `part12-z8m.md`'s `$9d24` -- re-verified byte-exact |
| `$11716`/`$1171e` (mask `$0f`, reroll on `==15`) | **NEW** | a rejection-sampling roll (uniform 1..15) reached when flag `$6c96` is set (`$116b0 tst.b $6c96; bne $11716`), bypassing the mode-1/2/3 dispatch `part12-dirkind.md` already opened entirely. Writes `(roll+1)` into `$6c60` -- the SAME mode-gate byte `part12-dirkind.md` reads to choose player 2's dir_kind/damage table vs. a hardcoded fallback -- then clears `$6da0`/`$6c97`/`$6c98`/`$6c9a`. |
| `$11746`/`$1174e` (mask `$0f`, reroll on `==15` OR on a slot already marked used) | **NEW** | mode 3's (tournament) OWN roll: clears a 16-byte "picked" table at `$6c86`, then samples WITHOUT REPLACEMENT from a 15-entry roster pool (reroll if the slot's own byte at `$6c86+roll` is already nonzero), marks the chosen slot, writes `(roll+1)` into `$6c60`, then reuses mode 2's own `$77de`+formula roster-pointer code to set `$6d96` -- confirmed by tracing the control flow precisely: `$116b0`'s guard, the mode-1/2/3 case chain at `$116b6`-`$1170e` (address-exact against `part12-dirkind.md`'s own citations), mode 3's `bra.w +$38` landing exactly on `$11746`, and the final `bra.s -$3a` from `$11716`'s tail landing exactly on mode 2's `$116dc`. |

The nine roll-or-reset call sites, each a read+write pair (2 raw hits)
except the reset (a plain move, 1 hit): `$968a` (reset, 1 hit); `$d2fa`,
`$da0e`, `$df8c`, `$d07a` (four dispatch/cascade rolls already named by
Part 12a/b, 2 hits each = 8); `$9ac6`, `$9af4`, `$9d26` (three bonus-mint
rolls, 2 hits each = 6); `$11716`, `$11746` (two mode-select rolls, NEW this
phase, 2 hits each = 4). `1 + 8 + 6 + 4 = 19` -- every one of the 19 raw
hits accounted for.

**What this means for reconstruction, restated precisely**: the two
mode-select rolls only run during character/mode setup, well before a
match's own frame 0 in every fixture on hand (and `$11716`'s own path needs
flag `$6c96`, not observed set in any fixture this project has captured).
The three bonus-mint rolls run on a fixed 20-tick gate this phase's own
oracle columns (`gate_6e3c`, `mint_6e3a`) expose directly, per-frame, in any
fixture. The four dispatch/cascade rolls are exactly `rng::eligible_rolls_
bound`'s own subject below. Nothing here retracts Part 12b's finding that
`$6c5d` cannot be PREDICTED from nothing before a fixture starts; it
completes the map of what a per-frame sample has to account for once one
does.

## 2. Emitting the stream

`oracle/disc-oracle.c`'s `emit_frame` gained five columns, all direct RAM
reads, no computed state:

```c
",\"rng_6c5d\":%u,\"latch_id\":%u,\"latch_prio\":%u"
",\"mint_6e3a\":%d,\"gate_6e3c\":%u,\"code_6d9e\":%d,\"code_6d9c\":%d"
```

* `rng_6c5d` -- the byte itself. `$6ab5` (the roll's stride) is NOT
  re-emitted as its own column: it is already `vbl_6ab4 & 0xff`, the low
  byte of a field the oracle has emitted since Part 10.
* `latch_id`/`latch_prio` -- `$6da6`/`$6daa`, the dispatch's OWN latch
  (identity function pointer, priority). Sampling these directly turns
  "which of the 20 rows is currently active" from something a model would
  have to infer from `$6da1`'s output bits into something a fixture states
  outright -- this is what makes §3 below a verification instead of a guess.
* `mint_6e3a`/`gate_6e3c`/`code_6d9e`/`code_6d9c` -- the bonus mint's own
  payload, its 20-tick reload gate, and the `$9aa2` table's consumable-count/
  duration fields (`docs/disc-notes.md`'s Part 10 table) for whichever code
  is active. These are §5's own subject.

`make -C oracle` rebuilds clean, no new warnings. Nothing committed was
regenerated; the six fixtures from before this phase are untouched.

## 3. Stream verification: the roll arithmetic is modelled, and it holds

`crates/disc-core/src/ai.rs` gained `pub mod rng`: the 20-row `(priority,
threshold, identity)` table from `part12-ai.md`, verbatim, plus
`eligible_rolls_bound(latch_priority, latch_identity)` -- an upper bound
(exact whenever no row preempts the latch mid-pass) on how many of the 20
rows reach the reaction roll in one `$d2cc` pass, and `delta_reachable`,
which checks whether an observed `$6c5d` delta is achievable via `k` rolls
of the known stride for some `k` in that bound.

`ai::rng_verify::bonus_code1_stream_within_bound` (a `#[test]`) replays
every consecutive frame pair in `tests/fixtures/bonus_code1.ndjson` (minted
this phase, §5) where the pass count is exactly one (`next.updates == 1`)
and the SAMPLED latch is unchanged across the pair (the one case the bound
is not exact for -- a row firing and un-latching again inside the same
pass, which the fixture's own `latch_id`/`latch_prio` columns make
detectable rather than assumed away):

```
bonus_code1 rng bound: 891 pairs checked, 70 skipped (latch changed), 138 skipped (multi-pass)
test ai::rng_verify::bonus_code1_stream_within_bound ... ok
```

**891 of 891 checked pairs pass. Zero unaccounted deltas.** Two shapes from
the checked set are worth citing on their own (measured separately, not
part of the committed test's own assertion, which only requires `checked >
0`):

* **64 pairs need exactly `k = 0`** -- every one of them a frame where the
  latch priority is already 50 (row 0, the escape, active), which
  `eligible_rolls_bound`'s own table admits no other row past. `$6c5d`
  provably cannot move on these frames under the decoded mechanism, and in
  the fixture, it doesn't.
* **3 pairs need the full `k = 20`** -- frames where nothing is latched
  (`latch_id == 0`), independently reproducing `reports/part12-rng.md`'s own
  LIVE finding ("with nothing latched... a single pass can burn on the
  order of twenty rolls at once") in a deterministic oracle run instead of
  a live Hatari trace.

One correction made getting here, worth stating precisely: the stride used
is `next.vbl_6ab4 & 0xff`, not `cur`'s -- `oracle/README.md`'s own sampling
contract (PC == `$8198`, BEFORE that instruction's `$6ab4` increment runs)
means the VBL during which the pass between two samples actually executed
is the one the LATER sample reports. Using `cur`'s own stride instead
looked, at frame 0->1, like `$6c5d` needed 242 rolls of a fixed stride to
explain a 1-frame transition -- a number nothing in the decoded mechanism
can produce, and a genuine, useful falsification signal for a stride
computed the wrong way round, not for the mechanism itself.

**This is the compute-verification the brief asked for.** It is a bound
check, not a full byte-for-byte replay -- an exact replay of every frame,
including ones where the latch DOES change, needs rows 6-17's own tests
decoded (§4), which this phase did not finish. What it establishes: the
`$d2cc` dispatch loop, as fully decoded in Part 12a, together with the
bonus mint's own gate (§1, this phase), completely accounts for every
observed `$6c5d` transition this fixture's 891 checkable frames show. No
residual, no unexplained consumer, no guess.

## 4. AI agreement: unchanged, and why that is the honest number

```
cargo test -p disc-core --lib ai::agreement -- --nocapture
```

| fixture | before | after |
|---|---|---|
| `golden.ndjson` | 18/99 | 18/99 |
| `tile_damage.ndjson` | 61/214 | 61/214 |
| `p1_walk.ndjson` | 22/200 | 22/200 |

**Unchanged, and correctly so.** Rows 0 and 1 -- the only two `Ai::p2_policy`
implements -- carry threshold 255, so their roll can never fail regardless
of `$6c5d`'s value (`part12-ai.md`'s own proof, restated in `ai.rs`'s module
docs): knowing the byte moves nothing for either row. Moving these numbers
needs rows 6-17 (or 2-5, 18-19) actually implemented, which needs their
TEST functions decoded, which is still blocked on the same two raw-byte
reads Part 12b left open (`$de12`, `$da84`) plus the two step types
(`$e37a`, `$e2b4`) neither row 0 nor row 1 ever compiles. §6 below reports
what this phase moved on that front (not enough to implement anything) and
cites the wall precisely rather than guessing past it.

## 5. `discr-z8m`: code 1's double-apply, measured

`tests/fixtures/bonus_code1.ndjson` (1100 frames, scripted -- full
provenance in its own `.provenance.md`): the first trace to catch bonus
code 1 minted (frame 644), picked up (frame 776, via the walk/strike-over
pickup `$a292`-`$a2ca` -- no damage, bit 7 stripped), and then CONSUMED
twice more (frames 992, 999) while still active.

The disc's own damage constant for this seed's character is established
directly from the SAME trace, not assumed: three hits (frames 107, 535,
656), all while an UNRELATED code (5, which gates the catch-reach mechanic
per `docs/disc-notes.md`'s `$9aa2` table, not damage) is active, each move a
cell's hp by exactly -3 (`4->1` twice, `5->2` once). A single such hit on an
hp-4 cell cannot reach 0. At frames 992 and 999, an hp-4 cell IS destroyed
outright by one strike, both while `bonus_6d9a == 1` is the active,
not-yet-spent effect -- `code_6d9e` (the code's own consumable count from
the `$9aa2` table) visibly decrements 5 -> 4 -> 3 on exactly those two
frames, the same signal `part12-z8m.md` read for code 3. The only damage
value consistent with `hp 4 -> 0` in one hit, given the established `-3`
baseline, is `-6`: the disc's damage applied twice -- `$a314`/`$a31c`'s own
decoded semantics, unexercised by any trace across three prior phases,
now measured.

`crates/disc-core/src/tile.rs` gained `bonus_damage_multiplier(bonus_code)`
(`2` for code 1, `1` otherwise) and a test,
`tile_bonus_code1::replays_every_hit_frame_exact`, that replays all five
hits above through the EXISTING `damage()` function with `base *
bonus_damage_multiplier(code)` and checks disc-core's own model reaches the
ST's own recorded hp, frame-exact:

```
frame 107: code=5 base=3 multiplier=1 hp 4 -> 1 (ST: 1)
frame 535: code=5 base=3 multiplier=1 hp 4 -> 1 (ST: 1)
frame 656: code=5 base=3 multiplier=1 hp 5 -> 2 (ST: 2)
frame 992: code=1 base=3 multiplier=2 hp 4 -> 0 (ST: 0)
frame 999: code=1 base=3 multiplier=2 hp 4 -> 0 (ST: 0)
test tile::tile_bonus_code1::replays_every_hit_frame_exact ... ok
```

**Not wired into `damage()`'s own signature or `disc.rs`'s call site this
phase** -- `disc.rs` is outside this session's lease (owned by `hook`/
whoever holds it this round; `bonus.md`'s own handoff already named the
exact one-line change needed: `tile::damage(tiles, cell, base *
tile::bonus_damage_multiplier(state.bonus_code), ...)`). `bonus_damage_
multiplier` is a free function precisely so that call-site change can land
without touching `damage`'s own signature or anything this phase does not
own.

**The mint recipe, for the record**: rather than 18 independent live-Hatari
boots (`part12-z8m.md`'s own recipe, ~4/18 hit rate), this phase used the
fast oracle itself: `--autopilot <cell> <period> <start>` sustains a real
rally (idle alone stalls the bonus gate by ~frame 300, well short of the
~130 gate-fires code 1 needs for reasonable odds at 2/128 per fire), and
because the oracle runs a ~1500-frame attempt in well under a second, a
sweep of 288 `(cell, period, start)` combinations against the SAME seed --
seconds of wall-clock, not 18 real boots -- found several that landed code
1 (first at `cell=11 period=4 start=10`, frame 644). The winning combo's
autopilot-driven joystick sequence was captured via `--emit-script` and
replayed with plain `--script` for the committed mint (byte-identical,
checked). This is the deterministic "compute forward, mint on purpose"
method the brief asked for, in the form it actually took: not hand-deriving
the roll sequence from a seed's own `$6c5d` (the mint gate's OWN cadence
turned out to be serviced far less than once per VBL once idle noise sets
in -- measured this phase, not assumed -- making closed-form prediction
brittle) but using the oracle's own speed to search a small, cheap parameter
space instead of a slow, live one.

## 6. Rows 6-17: incremental, not complete

`part12-rng.md` left two of six walk-cascade tests unread (`$de12`, `$da84`,
neither in Ghidra's instruction database -- 4 and 6 xrefs respectively, all
`DATA`) and two step types (`$e37a`, `$e2b4`) undecoded. This phase spent
bounded effort on `$de12` and got further than before, not to completion:

```
$de12  move.w $6d8a,d0
$de16  cmp.w  $6d8c,d0
$de1a  bge.w  $da3a                    ; bounds check (part12-rng.md's own
                                        ;  "same shape" claim, now address-exact)
$de1e  move.w $6ca2,d1                 ; player 1's world_x (part12-rng.md
                                        ;  already had this)
$de22  move.w $6ca6,d0                 ; player 1's world_y
$de26  cmp.w  #$14,d0                  ; #$14 = 20 (part12-rng.md already
$de2a  ble.s  $de42                    ;  had this, unresolved past it)
$de2c  bsr.w  $d9b8                    ; <-- NEW: the unresolved call,
                                        ;     address-pinned
```

`$d9b8` is itself the ENTRY to a cascade of further `bsr.w` calls (`$d7fa`
and at least one more past it, each preceded by a `cmp.w #$ffff,d0`/`beq`
"did the last candidate fail" check) -- structurally the same "try
candidate, fall through on failure" shape `part12-rng.md`'s own `$dbfc`/
`$db54`/`$dba8`/`$daf8` chain already named for a DIFFERENT helper, not (on
inspection) the same one. Resolving it needs at least two more levels of
the same raw-byte read this report and its predecessors all use; not
finished this phase, for the same reason Part 12b stopped where it did:
"stopped here rather than guess the branch target from hand-arithmetic on a
raw byte dump with no second source to check it against" -- now with one
more branch target pinned than before, not a name for what it leads to.
`$da84` was not attempted this phase (budget went to §1/§3/§5 instead, all
closure-grade for their own beads; this is the honest next-highest-leverage
item, restated for whoever picks it up).

`ai::agreement`'s three numbers are unaffected (§4) -- nothing here reached
a state safe to implement without guessing, per house rules.

## Closing both beads

**`discr-b6x`**: the brief's own reframed condition -- "the policy
compute-verified against fixtures, the honest per-pass-granularity bound
stated" -- is met. §1 completes the reader/writer map (19/19 raw hits
attributed, two entirely new consumers found). §3 compute-verifies the
decoded roll arithmetic against 891 real frame-pairs with zero residual,
including two extreme, independently-meaningful cases (the `k=0`
"nothing else can roll while row 0 is active" floor and the `k=20`
"nothing latched" ceiling, the latter reproducing a prior LIVE finding
deterministically). §4 states plainly that this does not move
`ai::agreement`'s numbers, and why (rows 0/1's roll was always
unconditional; the eighteen RNG-gated rows still need their own tests
decoded, §6, unchanged from Part 12b's own wall). Closing on the
compute-verification, not on 100% policy agreement -- that remains a
distinct, still-open unit of work (rows 6-17, and 2-5/18-19's own
sub-cases), narrower now than before this phase (§6) but not closed.

**`discr-z8m`**: the bead's own actual subject -- does `$6d9a==1` double a
struck tile's damage -- is measured for the first time in three phases,
in a committed, frame-exact-verified trace (§5), and the measured semantics
are implemented in `tile.rs` (`bonus_damage_multiplier`), ready for the
one-line `disc.rs` call-site change its own owner can land without needing
to re-derive anything.

## Gates

`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo
test` (61 tests, including the two new ones above) all clean. All ten
committed `tracecheck` gates re-verified passing at their existing
thresholds (99/214/99/214/274/21/222/150/22/34) -- no fixture's number
shrank. `bonus_code1.ndjson` is deliberately NOT wired into that fixed
ten-invocation list (its own numbers, 27/17, are recorded honestly in
`mise.toml` and its provenance, but reflect pre-existing, already-waived
gaps this fixture's new input shape reaches earlier than any other fixture
does -- not this phase's own regression, and not what this fixture exists
to measure; see its provenance's own "tracecheck" section).

## Files

* `oracle/disc-oracle.c` -- five new `emit_frame` columns (§2).
* `crates/disc-core/src/ai.rs` -- module docs updated; `pub mod rng` (the
  20-row table, `eligible_rolls_bound`, `delta_reachable`); `#[cfg(test)]
  mod rng_verify` (§3).
* `crates/disc-core/src/tile.rs` -- `bonus_damage_multiplier`; `#[cfg(test)]
  mod tile_bonus_code1` (§5).
* `tests/fixtures/bonus_code1.ndjson` (`git add -f`, gitignored like every
  other `*.ndjson`) + `tests/fixtures/bonus_code1.provenance.md`.
* `mise.toml` -- `BONUS_CODE1`/`BONUS_CODE1_MIN_AGREE`/
  `BONUS_CODE1_FULL_MIN_AGREE` (documentation only; not wired into
  `core-check`'s fixed gate list, see its own comment).
* `reports/part12-walls.md` (this file).
* Not committed (scratch, gitignored `tmp/`): `tmp/z8m_code1.script` (the
  committed fixture's own input programme -- reproducible via its
  provenance's own recipe).

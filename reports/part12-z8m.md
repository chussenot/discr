# Part 12c (z8m) — code 1/3 caught live; code 3 measured, code 1 still not

Agent `multiplier`. Bead `discr-z8m` (the `$6d9a` "damage multiplier").
Picks up where `reports/part12-bonus.md` left off: the writer of `$6d9a`
was already named there (`$a2b0`), but the bead's actual subject — does a
struck tile lose the disc's damage twice when `$6d9a==1` (`$a314`/`$a31c`),
and what does the `==3` path at `$a32e` do — had no trace exercising either
code, after three independent capture attempts and a 20,000-frame hunt all
rolled code 2.

## Summary

- **The `$9aa2` table (all five codes) and the mint distribution: already
  fully decoded** by prior agents (Part 10, `reports/part12-bonus.md`) and
  re-verified this session byte-for-byte against the raw image (not
  Ghidra's instruction model — see "Static re-verification" below). Odds
  computed and now empirically confirmed (see "The odds" and "The hunt").
- **The throughput fix**: a value-watch on `$6e3a` is blind to a reroll
  landing on the same code the byte already holds, which is *why* the prior
  20,000-frame hunt saw only 2 transitions against ~11/128 × (frames/20)
  expected successful rolls. Breaking on the mint instruction's own PC
  instead (`$9d58` for code 1, `$9d84` for code 3) catches every roll
  regardless of value. Measured this session: **code 1 and code 3 each
  caught multiple times, in 18 fresh-boot attempts** (histogram below) —
  the "lottery" was never as long a shot as the visibility bug made it
  look.
- **Code 3, measured live and clean**: three real tile hits while
  `$6d9a==3` (`$6d9e` visibly decrementing 3→2→1→0, the table's own
  "consumable count"), and **none of them doubles the damage** — one hit
  reads a clean `4→1` (the project's own established `-3` baseline, exactly
  once), the other two are clamped kills (`1→0`) which cannot rule doubling
  in isolation but are consistent with it. `$a32e`'s "further path" is
  measured, not guessed: on this evidence it does **not** double damage.
- **Code 1: caught four times, consumed zero times.** Every capture either
  had no completed pickup+damage trace (script bugs during this session,
  both now fixed and documented below) or — in the one clean full-window
  trace obtained (`catch_t10`) — the minted code was **overwritten by a
  later successful roll before any flagged tile was ever struck**, so
  `$6d9a` never actually became 1 in that window. `$a314`/`$a31c`'s
  double-apply is still **not** exercised by any trace. Per house rules,
  **not implemented**, and the bead **stays open**.
- **A previously-undocumented mechanic, found as a side effect**: an
  unconsumed bonus icon has a hard **250-frame lifetime** (`$9b1c: move.w
  #$fa,$6e38`) and expires on its own — bit 7 clears with **no** HP change
  and **no** `$6d9a` write — confirmed exactly (`catch_t10`: flagged at VBL
  6651, cleared at VBL 6901, delta 250; `catch_t8`: flagged at 6705, cleared
  at 6956, delta 251). A caught mint is not a caught *pickup*; the flagged
  cell has to be struck inside that window or the catch is wasted. This is
  the mechanism behind several of this session's near-misses and should be
  read alongside the hunt recipe below.

## Static re-verification (before touching Hatari)

`scripts/ghidra/q.sh dis 9d0c ...` reproduces the same failure mode already
named for `$efa8`/`$968a` in prior reports: it silently disassembles from
whatever function boundary its heuristics *do* recognize (landed at
`$9eea`, an unrelated function, not `$9d0c`) rather than erroring. So the
mint span was re-read byte-for-byte from `discram.bin` directly
(`od -A x -t x1z -j 0x9d0c -N 144 tmp/ghidra-mult/proj/discram.bin`,
decoded by hand against known 68000 opcodes the same way prior reports
decoded `$968a`), confirming every address prior reports already gave and
resolving them to the *exact* byte offsets needed for a PC breakpoint:

```
$9d0c  0c38 0001 6ca0        cmpi.b #1,$6ca0.w
$9d12  6700 0076             beq.w $9d8a          ; $6ca0==1 shortcut (below)
$9d16  5378 6e3c             subq.w #1,$6e3c.w
$9d1a  6600 006c             bne.w  $9d88         ; countdown not yet 0 -> skip
$9d1e  31fc 0014 6e3c        move.w #$14,$6e3c.w  ; reload only on the fire
$9d24  1038 6c5d             move.b $6c5d,D0
$9d28  d038 6ab5             add.b  $6ab5,D0
$9d2c  11c0 6c5d             move.b D0,$6c5d.w
$9d30  0200 007f             andi.b #$7f,D0
$9d34  b03c 0004             cmp.b  #4,D0
$9d38  6d00 fd7c             blt.w  $9ab6                    ; D0<4    (4/128) no bonus
$9d3c  b03c 0008             cmp.b  #8,D0
$9d40  6c0a                  bge.b  $9d4c
$9d42  31fc 0002 6e3a        move.w #2,$6e3a.w              ; 4<=D0<8  (4/128) code 2
$9d48  6000 fda0             bra.w  $9aea
$9d4c  b03c 000a             cmp.b  #$0a,D0
$9d50  6c0a                  bge.b  $9d5c
$9d52  31fc 0001 6e3a        move.w #1,$6e3a.w              ; 8<=D0<$a (2/128) CODE 1
$9d58  6000 fd90             bra.w  $9aea                   ; <-- breakpoint PC
$9d5c  b03c 000c             cmp.b  #$0c,D0
$9d60  6c0a                  bge.b  $9d6c
$9d62  31fc 0004 6e3a        move.w #4,$6e3a.w              ; $a<=D0<$c(2/128) code 4
$9d68  6000 fd80             bra.w  $9aea
$9d6c  b03c 000e             cmp.b  #$0e,D0
$9d70  6c0a                  bge.b  $9d7c
$9d72  31fc 0005 6e3a        move.w #5,$6e3a.w              ; $c<=D0<$e(2/128) code 5
$9d78  6000 fd70             bra.w  $9aea
$9d7c  660a                  bne.b  $9d88                   ; D0 != $e -> no bonus
$9d7e  31fc 0003 6e3a        move.w #3,$6e3a.w              ; D0==$e   (1/128) CODE 3
$9d84  6000 fd64             bra.w  $9aea                   ; <-- breakpoint PC
$9d88  4e75                  rts
```

Every mint branches to `bra.w $9aea` — a shared tail, byte-identical
displacement math confirmed for all five codes — so the instruction right
after the `move.w #code,$6e3a` is always that code's own dedicated `bra.w`,
giving a **code-specific PC** to break on that fires the instant (and only
the instant) that code is minted, with the write already landed:

```
CODE 1 mint write  $9d52   ->  breakpoint PC $9d58
CODE 3 mint write  $9d7e   ->  breakpoint PC $9d84
```

## The odds

D0 is masked to 0..127 (`and.b #$7f`) and compared against fixed bucket
boundaries — a **uniform 128-way roll**, not weighted beyond the bucket
widths already visible in the disassembly:

| bucket | width | odds | code |
|---|---|---|---|
| D0 < 4 | 4/128 | 3.125% | none |
| 4 ≤ D0 < 8 | 4/128 | 3.125% | 2 |
| 8 ≤ D0 < $a | 2/128 | 1.5625% | **1** |
| $a ≤ D0 < $c | 2/128 | 1.5625% | 4 |
| $c ≤ D0 < $e | 2/128 | 1.5625% | 5 |
| D0 == $e | 1/128 | 0.78125% | **3** |
| D0 ≥ $f | 113/128 | 88.28% | none |

Per gate-fire (the gate itself only fires once every 20 ticks of whatever
calls it, `$9d16`/`$9d1a`/`$9d1e`): **P(code 1) = 2/128 ≈ 1.56%**,
**P(code 3) = 1/128 ≈ 0.78%**, P(either) = 3/128 ≈ 2.34%. Conditioned on
*any* successful roll (11/128): P(code 1 | success) = 2/11 ≈ 18.2%,
P(code 3 | success) = 1/11 ≈ 9.1%.

The `$6ca0==1` branch at `$9d12` (labelled "special/test path,
unexercised" in every prior report) turned out to matter operationally
this session: when true it takes a completely different, **non-random**
path (`$9d8a: move.w #$0a,$6e3a` — a fixed code 10 the pickup table has no
row for) and skips `$9d16`'s countdown entirely. A savestate cache resumed
mid-idle can get stuck there indefinitely — measured directly:
`$6e3c` pinned at exactly 20 for 26,000+ VBLs in one such resumed session
(`tmp/diag_match_state.py`, not committed). Every hunt in this report uses
a **fresh boot** for this reason; a cached mid-match resume is not a safe
shortcut for this particular hunt (`$6ca0` is not confirmed constant, just
observed stuck at two different values, 1 and 512, in different sessions —
its own semantics are out of scope here).

## The hunt: fresh-boot, PC-breakpoint, `bra.w $9aea` catches the write with no polling

Recipe (`tmp/hunt_attempt.py`, not committed — scratch, same convention as
prior agents' `tmp/mint_*.py`/`tmp/hunt_code1.py`):

1. Fresh boot (`enter_match(cache=None, fresh=True, mode="challenge")`) —
   avoids the `$6ca0==1` stuck state above, and a fresh boot's own
   boot-to-menu timing is genuine new entropy for `$6ab5` (confirmed
   non-reproducible across boots in `reports/part12-rng.md`; this is what
   makes each attempt an independent draw, not what a reloaded savestate
   would give).
2. Arm both breakpoints **before** playing, non-blocking, each `:once
   :trace :file <script>` (the same "no round-trip" trick
   `Hatari.seed()` already uses for the VBL handler, here at the mint's own
   PC instead): the `:file` script does `savebin $0 $8000` + `r` (registers)
   + the MFP shadow, so a catch is a complete oracle-style seed with zero
   polling latency.
3. Drive `scenarios/bonus_hunt.yaml`'s own proven rally (hold Fire 3 / wait
   60 / hold Right 20 / wait 25 / hold Left 20 / wait 25, repeated), which
   is what got the very first `$6e3a` writer caught in
   `reports/part12-bonus.md`. Stop as soon as either capture file exists,
   or as soon as `in_match()` goes false (a CHALLENGE round is short —
   measured ~150-900 live VBLs before it ends; there is nothing left to
   catch once it does, see below).

**Histogram, 18 fresh-boot attempts this session** (mode=challenge,
`scripts/collect.py`'s own `in_match()` screen check, `~27-90s` wall-clock
each):

| attempt | result | note |
|---|---|---|
| 1 | miss | |
| 2 | **code 1** | mint only (script stopped there) |
| 3 | miss | ~48 gate windows |
| 4 | **code 1** | mint only |
| 5 | miss | ~60 gate windows |
| 6 | miss | ~34 gate windows |
| t1 | **code 3** | mint + trace, but the trace *drove* Right/Left throughout and got zero tile-bank activity — driving input during the trace interferes with the disc's own rally, see fix below |
| t2 | miss | |
| t3 | **code 1** | mint only — continuation trace lost (fast-forward bug, see below) |
| t4 | miss | |
| t5 | miss | |
| t6 | **code 3** | mint only — continuation trace lost (same bug) |
| t7 | miss | |
| t8 | **code 3** | **clean full trace — see "Code 3 measured" below** |
| t9 | miss | |
| t10 | **code 1** | **clean full trace — mint confirmed, never consumed, see below** |
| t11 | miss | |
| t12 | miss | |

**7/18 caught either code (38.9%)** — code 1 four times, code 3 three
times. Empirically consistent with the computed odds (each attempt's live
window ran somewhere around 30-90 gate-fire windows before the round
ended; P(catch) over that range spans roughly 50-90%, and 7/18 sits
squarely inside the combined variance of 18 independent draws at those
per-attempt rates).

**Two bugs found and fixed mid-session, both address-worth noting for
whoever runs this next:**

- **Driving input during the post-catch trace can kill the rally.**
  `catch_t1` continued issuing `hold(Right)`/`hold(Left)` during the whole
  trace window and saw *zero* tile-bank writes over 381 frames, despite a
  bonus icon (cell 4, both banks) sitting flagged the entire time. Fix:
  after the catch, stop driving new input and let the disc that is already
  being volleyed keep flying under the game's own AI — `catch_t8`/`catch_t10`
  do this and both show real tile hits.
- **Fast-forward must be off during `frame_trace`.** Its own sleep budget
  (`frames * 0.02 * 1.15 + 0.3` real seconds) assumes roughly-normal
  emulation speed. Leaving fast-forward on for the post-catch trace (as
  `catch_t3`/`catch_t6` did) let two attempts race through a
  post-round idle/reload screen — far cheaper to render than live play, so
  fast-forward ran far faster there — and produced **1.1GB single-run JSON
  traces of nothing but idle frames** (deleted, never committed). Fix:
  `h.fast_forward(False)` immediately before calling `frame_trace`;
  `catch_t8`/`catch_t10` (900-frame budget) produced normal ~48MB traces
  with sane per-VBL content.

## Code 3, measured (`catch_t8`)

Full per-VBL trace from the instant of the catch (`$9d84`) through 1054
VBLs, idle (no driven input, disc already in flight). `$6d9a`/`$6d9e`
across the window:

```
VBL 6590  6d9a=3 6d9e=3   (code 3 already active+picked up by trace start --
                            the flagged cell from an earlier roll got struck
                            in the gap between the breakpoint firing and the
                            trace's first sample)
VBL 6606  6d9a=3 6d9e=2   near[6]: hp 1 -> 0   (a kill; clamped, inconclusive alone)
VBL 6685  6d9a=3 6d9e=1   near[7]: hp 4 -> 1   (a CLEAN single hit: -3, the
                                                 project's own established
                                                 damage baseline, exactly once)
VBL 6695  6d9a=0 6d9e=0   near[7]: hp 1 -> 0   (kill; 6d9a cleared same VBL)
```

`$6d9e` is exactly the table's code-3 "consumable count" (3, per the
`$9aa2` table already in `docs/disc-notes.md`), visibly decrementing once
per hit while `$6d9a==3`, clearing both to 0 the moment it exhausts — this
alone confirms the table's own semantics live, independent of the damage
question.

**The damage question, answered**: the `near[7]` hit at VBL 6685 is a
clean, unambiguous **single** `-3` (`4 -> 1`), not a double (`4 -> -2`,
clamped 0). Both other hits are kills (clamped at 0 either way, so they
cannot independently rule out doubling) but are fully consistent with
ordinary single damage. **`$a32e`'s "further path" for code 3, on this
evidence, does not double tile damage.** This is a measurement, not an
inference from the disassembly: three real hits, one of them unambiguous.

(The trace also incidentally caught a *second*, unrelated bonus icon
placed at `near/far[2]` VBL 6705 and expiring — bit 7 cleared, hp
unchanged, `$6d9a` untouched — at VBL 6956, delta 251: the 250-frame
icon-lifetime mechanic above, confirmed a second time.)

## Code 1, still not measured (`catch_t10`)

Same clean-trace method, code 1 this time. The catch itself is solid
(post-mint RAM: `$6e3a=1 $6e3c=20 $6d9a=0`), but the trace shows the
mint **never reaching a pickup**:

```
VBL 6544  6e3a=1                          (our catch)
VBL 6587  far[4]: hp 4 -> 1                (an ordinary hit, unrelated cell,
                                             6e3a still 1, unconsumed)
VBL 6651  6e3a: 1 -> 3                     (A NEW roll fires and OVERWRITES
          near/far[1]: hp 4 -> 132/133      $6e3a before code 1 is ever
                                             consumed -- the mint write at
                                             $9d52/$9d7e is unconditional on
                                             $6e3a's current value, so an
                                             unconsumed code is not safe from
                                             being clobbered by the next
                                             successful roll)
VBL 6901  (icon lifetime expires, 250 frames after 6651)
VBL 6902  near/far[1]: hp 132/133 -> 4     (bit 7 cleared, hp UNCHANGED --
                                             the icon expiry mechanic, not a
                                             pickup; 6d9a stays 0 throughout)
```

Code 1 was minted and then **silently overwritten by a later roll before
any tile carrying it was ever struck** — `$6d9a` never left 0 in this
entire window. This is a genuine, measured near-miss, not a null result:
the catch mechanism works exactly as designed (caught the write instantly,
confirmed byte-for-byte), but a caught mint is not yet a caught *pickup*,
and this window shows concretely how a catch gets wasted (reroll, or the
250-frame expiry, whichever comes first).

**`$a314 cmp.w #1,$6d9a` / `$a31c`'s double-apply is therefore still not
exercised by any trace this session or before it.** Per house rules — a
decoded-but-unexercised path is documented, not implemented — `disc-core`
is unchanged, and **discr-z8m stays open**.

## What would close it from here

The recipe is proven twice over (code 1 four separate catches, code 3
three, in 18 boots) and the failure mode for a caught-but-unconsumed mint
is now understood precisely (reroll before pickup, or 250-frame expiry
before pickup — both measured, not guessed). What's missing is arranging
for the disc to actually strike the *newly flagged* cell before either of
those; the current recipe catches the mint but does nothing to steer play
toward the flagged cell afterward. Concretely: after the catch, read which
cell `$9aea`'s own roll flagged (`(D0 & 7) + 1`, sampled right after the
breakpoint releases) and drive input to bias play toward that cell, or
simply run more attempts — at ~4/18 code-1 catches this session, a
handful more boots has good odds of landing one where the flagged cell
happens to already be in the disc's path.

## Files

- `tmp/hunt_attempt.py`, `tmp/hunt_catch_trace.py` (kept local, not
  committed — scratch drivers, same convention as prior agents' `tmp/mint_*.py`;
  reproducible from the addresses and recipe above in a fresh worktree)
- `tmp/analyze_trace2.py`, `tmp/analyze_hits.py`, `tmp/peek_frame.py`,
  `tmp/peek_mint.py` (scratch analysis helpers, not committed)
- Captured evidence, this worktree only (gitignored `tmp/`, not committed —
  absolute paths for whoever picks this up next):
  - `tmp/hunt13/catch_t8/code3_mint.bin` + `.json` (seed) and
    `trace_code3.json` (1054-frame clean trace) — **the code-3
    measurement above**
  - `tmp/hunt13/catch_t10/code1_mint.bin` + `.json` and `trace_code1.json`
    (1054-frame clean trace) — **the code-1 near-miss above**
  - `tmp/hunt13/attempt_2/code1_mint.bin`, `attempt_4/code1_mint.bin`,
    `catch_t1/code3_mint.bin`, `catch_t3/code1_mint.bin`,
    `catch_t6/code3_mint.bin` — mint-only catches, no consumption trace
- `scenarios/bonus_code1_hunt.yaml` (committed — the underlying
  reach-a-live-challenge-round-and-rally scenario the PC-breakpoint driver
  builds on; the breakpoint arming itself is not expressible in
  `collect.py`'s scenario DSL, which only supports value-watches, so it
  lives in the `tmp/` driver above, not the YAML)
- `docs/disc-notes.md`: none needed — the `$9aa2` table and mint
  disassembly it already carries (added by Part 10/12) are confirmed
  byte-exact by this session's independent re-read, no changes required

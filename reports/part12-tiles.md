# Part 12 (tiles) — a re-measurement, four collapse slots, and a $6d9a watch

Three beads: `discr-qsf` (re-measure a recorded divergence before believing
it), `discr-pu8` (model all four collapse slots, not one), `discr-z8m`
(stretch: what writes `$6d9a`). Every claim below cites a command run in this
worktree and an ST address.

    cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/p1_walk.ndjson [--skip-waived]
    GHIDRA_HOME=... GHIDRA_PROJ=/tmp/ghidra-tiles/proj ./scripts/ghidra/q.sh dis a370 60 xref 779e ...
    python3 scripts/collect.py --scenario scenarios/watch_6d9a_rally.yaml -v

## discr-qsf: the frame-238 divergence is gone, and here is the commit that fixed it

Re-measured exactly as the bead's own reproduce line says:

```
$ cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/p1_walk.ndjson
OK: 274 tick(s) matched, no divergence.

$ cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/p1_walk.ndjson --skip-waived
OK: 274 tick(s) matched, no divergence.
```

Both clean. The bead was filed at commit `400606e` ("frame-238 bead filed"),
before Parts 11h/11i/11j landed. The fix is `b66be7a` ("the collapse advances
per outer iteration, not per frame -- 237 -> 255"), which names this *exact*
bug in its own commit message:

> `$96cc bpl` branches back to `$96be`, not `$96b6`, so everything above that
> target runs once per OUTER main-loop iteration... Priced with `--watch
> 0x779e 0x77b0`: the ST clears cell 14 eighty-five frames after destroying
> cell 6, where one step per frame takes 48. **That was p1_walk's frame-238
> wall.**

`disc-core` had been advancing the collapse animation once per sampled
frame instead of once per `$96b6` outer-loop iteration, so it raced ahead of
the ST and cleared `tiles[14]` at frame 238 — 47 frames early. `b66be7a`
fixed the pacing (237 -> 255), and it is also where the four-slot retraction
(`discr-pu8`) originates. `7dba893` (Part 11i, 255 -> 271) and `19ac647`
(Part 11j, 271 -> 274, "all three fixtures clean for the first time") carried
`p1_walk` the rest of the way. Closed with this citation; see
`bd show discr-qsf` for the full comment.

## discr-pu8: the collapse is four slots, and $a38c does scan

### Static analysis (Ghidra, `tmp/ghidra_proj`, `scripts/ghidra/q.sh`)

The claim loop, read in full:

```
$a386  moveq  #3,D6
$a388  lea    $779e.w,A2
$a38c  tst.b  (A2)          ; [loop top]
$a38e  bne.b  $a3b2         ; busy -- try the next slot
$a390  st     (A2)          ; free -- CLAIM it
$a392  lea    ($58,PC),A0
$a396  move.l (0,A0,D5w),(8,A2)
$a39c  move.l (4,A0,D5w),(0xc,A2)
$a3a2  clr.w  (2,A2)
$a3a6  lea    $7656.w,A0
$a3aa  adda.w D5w,A0
$a3ac  move.l A0,(4,A2)
$a3b0  bra.b  $a3ba         ; claimed -- done
$a3b2  lea    ($10,A2),A2   ; next slot, $10 (16) bytes on
$a3b6  dbf    D6w,$a38c
$a3ba  bsr.w  $a434         ; (unrelated: a different 4-slot table at $770e)
```

`fun` confirms this is one function, `sub_a354`, 152 bytes (`$a354`-`$a3ec`).
`moveq #3,D6` gives exactly four `dbf` iterations over `$779e`/`$77ae`/`$77be`/
`$77ce`, stride `$10` — matching the bead's own description byte for byte.

**The bead's open question is answered**: `$a38c` scans all four slots for the
first one whose byte reads 0, claims that one, and stops. It does *not* queue
behind a busy slot, and it does *not* merely test `$779e`. If the `dbf`
exhausts all four without finding one free, execution falls through to `$a3ba`
unclaimed — the destroy's collapse animation is silently dropped. That
fall-through case is new in this model (see `a_fifth_destroy_with_all_slots_busy_drops_its_collapse`
in `tile.rs`).

**Caveat, not a retraction**: the pre-existing citation of `$a4bc` for the
*advance* loop (the one `$96b6` calls once per outer iteration, walking all
four slots and `jsr $14ba4` on each busy one) could not be re-confirmed in this
Ghidra project snapshot — `$a4bc` falls in a span with no defined Ghidra
function and zero xrefs to `$14ba4`; `fun` lists `FUN_0000a434` (ending
`$a472`) and `sub_a4ea` (starting `$a4ea`) with nothing named in between. Either
that region needs auto-analysis re-run, or the address was transcribed from an
interactive Ghidra session this batch snapshot doesn't carry. Left as-is: the
claim-side addresses match exactly, and the advance logic itself
([`collapse_step`]) is unchanged except for the number of slots it now walks.

### The four-slot model

`crates/disc-core/src/tile.rs`:

* `COLLAPSE_SLOTS: usize = 4`, from `$a386 moveq #3,D6`.
* `Collapse` (the per-slot struct) is unchanged.
* `damage()`'s collapse parameter is now `&mut [Option<Collapse>;
  COLLAPSE_SLOTS]`; the claim is `collapse.iter_mut().find(|s| s.is_none())`
  — first free slot in array order (`$779e`, `$77ae`, `$77be`, `$77ce`),
  matching the disassembly. No free slot -> does nothing, matching the ST's
  silent drop.
* `collapse_step()` now iterates all four slots per call, running the
  existing single-slot advance logic (unchanged) on each occupied one.

`crates/disc-core/src/lib.rs`: `GameState::collapse` is now `[Option<tile::Collapse>;
COLLAPSE_SLOTS]` (was `Option<tile::Collapse>`), `Default`-initialised.

`crates/disc-core/src/disc.rs` (shared with "owner", leased with `--wait`):
`disc::step` and `disc::impact`'s `collapse` parameters retyped to match; the
16 `&mut None` test call sites became `&mut [None, None, None, None]`.

Two new tests in `tile.rs`: `a_fifth_destroy_with_all_slots_busy_drops_its_collapse`
and `the_claim_takes_the_first_free_slot_in_order`.

**Behavior-preserving**: modelling one slot was correct only while no trace
destroys two tiles within 50 collapse steps, and none of the three committed
fixtures does. All 6 gates below are green with the *same* numbers as before
landing this — nothing regressed by going from one slot to four.

### A two-collapse fixture: attempted, not cheap, recipe recorded

Built `oracle/disc-oracle` and copied `seeds/match_challenge.seed` into this
worktree (gitignored, not committed). Tried three input programmes over
watches on `$7616`-`$77ce`:

| script | frames | destroys observed |
|---|---|---|
| idle | 900 | 1 (frame 225, cell `$764e`) |
| hold Fire from frame 5 | 600 | 1 (frame 225, cell `$764e`) — identical |
| hold Right+Fire from frame 5 | 400 | 1 (frame 225, cell `$764e`) — identical |

All three produce the *same* single destroy at the *same* frame: a short
scripted hold doesn't perturb which tile the opponent's own play destroys
(consistent with `tests/fixtures/tile_damage.provenance.md`'s note that its
one destroy "comes from the opponent's play", no input programme needed).
Getting two destroys within 50 steps needs a script that lands a scored hit on
each floor half inside a ~50-frame window — that requires aiming both discs
deliberately, which is a volley-engineering problem, not a one-line script.
**Not cheap** with the tools tried in the time available.

Recipe for whoever picks this up: watch `$7616`-`$7666` (the 8 damageable
cells, `docs/disc-notes.md`'s "bank is eight tiles held twice") over a long
idle+volley capture to find where destroys land naturally, then script
joystick input (`scripts/regen_golden.sh`'s frame-tagged `j` lines,
`oracle/README.md`'s script grammar) to land a second hit near an existing
one. `scenarios/watch_6d9a_rally.yaml` (this report, `discr-z8m` section) is
a working scaffold for the Hatari side if a live-emulator approach is
preferred to the oracle.

Closed with both conditions met: the four-slot model landed with all gates
green, and the `$a38c` question answered from the disassembly. The
two-collapse fixture is a recorded recipe, not a delivered file, per the
bead's own "if not cheap, write the recipe" allowance.

## discr-z8m (stretch): $6d9a watch

Scenario: `scenarios/watch_6d9a_rally.yaml`, `mode: training` (no bonus board,
so the `$9aa2` bonus-table writer the bead names as a suspect cannot fire —
any writer this run finds is not that one). Serves the disc, then alternates
Left/Right holds through several volleys so the rally continues instead of
one shot dying immediately, watching `$6d9a` (word) the whole time.

**Result: `[watch] $6d9a: 0 hit(s) from PC ` — zero writes across the whole
window.** Confirmed live, not a menu or paused screen: `collect.py`'s own
pixel-based `in_match()` check passed ("`[nav] match live after 3 fire(s)`"),
and the on-screen round clock visibly decremented `00:56 -> 00:53 -> 00:50`
across three screenshots taken during the watch window
(`tmp/shots-watch_6d9a_rally/{mid1,mid2,post}.bmp`) — proof frames were
actively advancing, not sitting on a static screen. The watch was armed for
~389 frames (`settle 60` + two Fire/Right/Left volleys + a final Fire and a
60-frame tail), with Fire held twice (attempting to serve) and Right/Left held
four times (attempting to move and return).

**Caveat, stated plainly**: the player sprite sits stationary center-court and
the disc HUD icon holds the same position in all three screenshots — three
sparse samples over 389 frames cannot by themselves confirm a disc was served
and volleyed, as opposed to sitting racked, even though the round clock proves
the simulation itself was running live. A stronger run would `--dump`/`--trace`
the disc record (`$6e3e`+) to directly confirm `disc.active`/`world_x`/
`world_y` changed inside the window, or sample a screenshot every ~10 frames
instead of 3 over the whole run.

The bead's named suspect, the bonus-pickup table at `$9aa2`, is ruled out as
the explanation for *this* null by mode alone: training has no bonus board, so
that writer cannot fire here regardless of what the rest of the round did.
`$6d9a`'s actual writer is still unlocated. `discr-z8m` stays **open**, per its
own "stays open unless a writer is named" — this is recorded as a measurement,
not a solve.

## Files touched

* `crates/disc-core/src/tile.rs` — four-slot `Collapse` model, two new tests.
* `crates/disc-core/src/lib.rs` — `GameState::collapse` retyped.
* `crates/disc-core/src/disc.rs` — `step`/`impact` signatures retyped (leased
  from "owner" with `--wait`, released after committing).
* `scenarios/watch_6d9a_rally.yaml` — new scenario, not committed by this
  agent (the `scenarios/` directory is "owner"'s lease); handed off in this
  report and the `discr-z8m` bead comment for "owner"/the orchestrator to
  land if it's wanted in-tree.
* `reports/part12-tiles.md` — this file.

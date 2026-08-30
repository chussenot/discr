# Part 12 (owner): disc+$11 owner polarity, and the four possession counters

bd discr-ovl.2. Every claim below cites an ST address, and either a Ghidra
static disassembly or a measured trace (usually both).

## The question

`docs/disc-notes.md` ("The disc update loop `$a4ea`, in full", Part 10) named
the wall handlers that flip `disc+$11` (the disc's owner byte) and move four
counters, `$6d8a`/`$6d8c`/`$6d0a`/`$6d0c`, but at the time every trace on hand
read owner `0` on every live slot -- no trace had ever seen a disc change
hands, so `tracecheck`'s `t.own == 0 -> PlayerId::One` mapping was a labelled
guess. The acceptance for this bead: a committed trace with disc+$11
non-zero, and the four counters' direction named against it.

## What already existed: `p1_walk.ndjson` has the first half

Part 11j (already committed, `tests/fixtures/p1_walk.ndjson`) found disc slot
0's owner byte move 0 -> 255 at frame 220, at the far wall (`world_z` crossing
79, `dir_kind` flipping from +1 to -1). That alone already satisfies the
literal acceptance criterion. This part:

1. Confirms every writer PC involved with a fresh Ghidra read (the prior notes
   had an internal inconsistency worth flagging -- see below).
2. Mints a second fixture, `tests/fixtures/handover.ndjson`, that catches
   **both** directions on the same disc slot, so the counter symmetry can be
   checked against data instead of asserted from the disassembly alone.
3. Names which REAL player owns which raw byte value -- not attempted before.
4. Empirically tests whether `tracecheck`'s guess can simply be flipped (it
   cannot, without a coordinated disc-core change -- see "What was NOT
   changed, and why").

## The writer sites, re-verified via Ghidra

Fresh disassembly (`scripts/ghidra/q.sh dis <addr> <n>`, this session,
`/tmp/ghidra-owner/proj`), not reused from old notes:

**Serve / spawn** (`$a99c`-`$a9c8`, entered only via `$a972`, itself called
exclusively from inside player 2's control routine `$abb2` -- see
`docs/disc-notes.md` line ~972, "player 2's control routine"):

```
$a9aa  addq.w #1,$00006d8a.w     ; bump P2's own outstanding-disc count
$a9ae  move.l D0,(A1)            ; world_x/world_y
$a9b0  move.l D1,(4,A1)          ; world_z/vel_x
$a9b4  move.l D2,(8,A1)          ; vel_y/dir_kind
$a9b8  st     (0x10,A1)          ; active := $FF
$a9bc  clr.b  (0x11,A1)          ; OWNER := 0, in the same routine
```

So: **every disc, at the instant it is served, has owner forced to 0 and
simultaneously bumps `$6d8a`** -- and only player 2 ever serves in this game
mode (one-player training/challenge; `$6d8c` reads 0 for player 1 at round
start, "so player 1 can never throw from this state" -- `docs/disc-notes.md`,
Part 10g -- consistent with $6d8a's cap being a player-2-only resource until a
transfer happens).

**Far wall** (`$a5ba`-`$a5fa`, `world_z` crossing 79), gated on the owner byte
and `$6ca0.b != 1`:

```
$a5d0  tst.b  (0x11,A5)
$a5d4  beq.b  $a5e2                       ; owner == 0 -> the transfer path
$a5d6  move.w #-1,(0xa,A5)                ; owner != 0: dir_kind := -1
$a5dc  bsr.w  $9f5e                       ; far tile grid (discr-ovl.3)
$a5e2  cmpi.b #1,(0x00006ca0).w
$a5e8  beq.b  $a5fe                       ; gate: skip if $6ca0.b == 1
$a5ea  st     (0x11,A5)                   ; owner := $FF
$a5ee  subq.w #1,(0x00006d8a).w
$a5f2  subq.w #1,(0x00006d8c).w
$a5f6  addq.w #1,(0x00006d0c).w
$a5fa  addq.w #1,(0x00006d0a).w
```

**Near wall** (`$a5fe`-`$a63c`, `world_z` crossing 0), the exact mirror:

```
$a612  tst.b  (0x11,A5)
$a616  bne.b  $a624                       ; owner != 0 -> the transfer path
$a618  move.w #1,(0xa,A5)                 ; owner == 0: dir_kind := +1
$a61e  bsr.w  $a24c                       ; near tile grid
$a624  cmpi.b #1,(0x00006ca0).w
$a62a  beq.b  $a640
$a62c  clr.b  (0x11,A5)                   ; owner := 0
$a630  subq.w #1,(0x00006d0a).w
$a634  subq.w #1,(0x00006d0c).w
$a638  addq.w #1,(0x00006d8c).w
$a63c  addq.w #1,(0x00006d8a).w
```

This directly **contradicts** one older note in `docs/disc-notes.md` (Part
10g, "`$6d8c` is the cap on `$6d8a`, never written anywhere in the image"):
`$a5f2` and `$a638` both write it. That claim was about a *static* value read
in one seed, not a search of the whole image; it is superseded by the above
and should be read as retracted (leaving it as historical record rather than
editing Part 10g's prose, per house rules about not rewriting old findings
in place).

## Which REAL player owns which raw value

Three independent lines of evidence, all agreeing:

**1. The server's identity.** Every serve is issued from inside player 2's
own control routine (`$abb2`/`$a972`), and the SAME routine that bumps
`$6d8a` also clears the owner byte (`$a9aa` then `$a9bc`, four instructions
apart, no branch between them). A disc cannot be served with any owner value
other than 0, and it cannot be served by anyone but player 2. So owner `0` is,
from birth, charged against player 2's own resource ledger.

**2. `$6d8a`/`$6d8c` are demonstrably player 2's own fields elsewhere in the
code.** `$c196` (state 18, player 2's own intercept-commit handler --
`docs/disc-notes.md` line ~1621) reads `if $6d8a == $6d8c -> out` to decide
whether **player 2** may throw again; so does `$ad92` in player 2's idle-path
throw decision. Both are textually inside player 2's own control routine.

**3. Live confirmation, both directions, same disc slot** --
`tests/fixtures/handover.ndjson`:

```
frame 258 -> 259  (FAR wall; disc[1].own 0 -> 255)
  players[0] (P1) discs_out,disc_cap:  0,0 -> 1,1   (UP)
  players[1] (P2) discs_out,disc_cap:  3,4 -> 2,3   (DOWN)

frame 338 -> 339  (NEAR wall; disc[1].own 255 -> 0)
  players[0] (P1) discs_out,disc_cap:  1,2 -> 0,1   (DOWN)
  players[1] (P2) discs_out,disc_cap:  0,2 -> 1,3   (UP)
```

`players[n].discs_out`/`disc_cap` are already-modelled trace columns; the
index convention (`players[0]` = player 1, `players[1]` = player 2) is
`disc_core::PlayerId::index()` (`crates/disc-core/src/types.rs`). This
matches the disassembly exactly: player 2's pair goes down and player 1's
goes up at the far wall (`$a5ee`/`$a5f2` subtract, `$a5f6`/`$a5fa` add), and
the reverse at the near wall.

**Verdict: raw owner `0` is PLAYER 2's disc. Raw owner `0xFF` (255) is PLAYER
1's disc.** A disc is born (served) as player 2's, and stays charged to
player 2's throw budget until it survives all the way to player 2's OWN far
wall uncaught, at which point it transfers to player 1's ledger; the near
wall reverses the transfer.

Both transfer gates additionally require `$6ca0.b != 1` -- that byte (an
apparent flag on player 1's own record, `$6ca0` = player 1 + 0) is not
decoded. It is very likely part of round-end/scoring bookkeeping (discr-st8)
but nothing here ties it to score directly; flagged, not chased further.

## What was NOT changed, and why: the current code guess is functionally load-bearing

The obvious next step reads as "flip `crates/disc-tools/src/main.rs`'s
`t.own == 0 -> PlayerId::One` to `PlayerId::Two`." I tried it and measured
the result before deciding:

```
$ cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/p1_walk.ndjson --min-agree 274
# baseline (current mapping): OK: 274 tick(s) matched, no divergence.
# after flipping ONLY the aim mapping's two arms:
DIVERGENCE at trace frame 11 ... discs[0].hook expected 42968 got 0
FAIL: --min-agree 274 but only 10 tick(s) matched -- a regression.
```

The reason: `disc.aim` (`crates/disc-core/src/disc.rs`, `player.rs`) is
**fed every tick, never compared** (`feed_disc_inputs`, `main.rs`) --
disc-core has no writer for the owner byte at all. But its OWN internal logic
still branches on `disc.aim == PlayerId::One` in several places (the
near/far-wall match arms in `disc.rs`, and the per-player anticipation-cascade
gate in `player.rs` around lines 1055-1061), written under the assumption
that raw owner `0` maps to `PlayerId::One`. That assumption is self-consistent
today -- it correctly reproduces which player's cascade fires when, because
both the feed and the internal checks agree with each other, even though
neither agrees with which REAL player raw `0` belongs to. Flipping only the
feed desyncs the two, which is exactly what regressed p1_walk.

So the mapping in `main.rs` is unchanged (still `0 -> PlayerId::One`); what
changed is the comment next to it, which now cites the real polarity and
explains precisely why the code is not flipped to match it. Filed
**discr-ovl.8** for the coordinated fix (flip `main.rs`'s mapping AND every
internal `disc.aim == PlayerId::One`/`Two` check in `disc.rs`/`player.rs`
together, since `PlayerId::One`/`Two` is the same enum used everywhere else
to mean the real players and should not lie for this one field). Messaged
the current owners of `disc.rs` (tiles) and `player.rs` (states), since the
fix touches files this bead does not own.

## The fixture: `tests/fixtures/handover.ndjson`

450-frame oracle trace, seed `seeds/match_challenge.seed` (freshly minted
this session via `scenarios/oracle_seed.yaml` against a live Hatari boot;
sha256 `0b23f4d6583b5c80386adf5ef210dd84728775c39bfee8f5bc9bb98f003779f9`,
`$6ab4` = 6331), same input programme as `p1_walk` (player 1 walks left
frames 5-30, then idle; player 2 is the AI). Reusing that exact programme
against a fresh seed is what caught the return leg `p1_walk` never reached --
same recipe, longer window (450 vs 275 frames), different slot.

It is **not** offered as a new reproduction-length record: `disc-core`'s own
replay of it diverges early (tick 21 bare, tick 222 with `--skip-waived`) on
`discs[0].active`, an existing gap in the active-byte retirement countdown
unrelated to this bead (a disc caught and counted down in the same tick --
another multi-update-pass tick in the Part 11f/11g sense, this time hitting a
different field). The fixture's evidentiary value is in its own recorded
columns at frames 259 and 339, which do not depend on how far disc-core's
replay gets. Full detail: `tests/fixtures/handover.provenance.md`.

Gated in `mise.toml` (`HANDOVER_MIN_AGREE=21`, `HANDOVER_SKIP_MIN_AGREE=222`),
added to `core-check`/`tracecheck`/`tracecheck-deep`, matching the existing
convention for the other three fixtures.

## Files touched

* `tests/fixtures/handover.ndjson` (new, `git add -f`) + `.provenance.md`
* `mise.toml` -- new `HANDOVER`/`HANDOVER_MIN_AGREE`/`HANDOVER_SKIP_MIN_AGREE`
  vars, wired into `core-check`, `tracecheck`, `tracecheck-deep`
* `crates/disc-tools/src/main.rs` -- comments only (three sites), citing the
  resolved polarity and explaining why the mapping itself is unchanged
* `docs/state-schema.md` -- the `discs[n].aim` waiver's prose updated with
  the citation; still `waived:discr-ovl.2` (fed, not compared -- that has not
  changed) rather than un-waived, since disc-core still has no writer for it
* `reports/part12-owner.md` (this file)
* bd: comment on `discr-st8` (the four counters' writers, which unblocks its
  "RE of the writers" precondition); new bead `discr-ovl.8` (the coordinated
  PlayerId-consistency fix)
* pact: message to `tiles`/`states` (current owners of `disc.rs`/`player.rs`)

## Gates, before landing

```
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test        # clean
tracecheck golden.ndjson       --skip-waived --min-agree 99   -> OK, 99
tracecheck tile_damage.ndjson  --skip-waived --min-agree 214  -> OK, 214
tracecheck golden.ndjson       --min-agree 99                 -> OK, 99
tracecheck tile_damage.ndjson  --min-agree 214                -> OK, 214
tracecheck p1_walk.ndjson      --min-agree 274                -> OK, 274
tracecheck handover.ndjson     --skip-waived --min-agree 222  -> OK, 222 (new)
tracecheck handover.ndjson     --min-agree 21                 -> OK, 21  (new)
```

No existing number shrank.

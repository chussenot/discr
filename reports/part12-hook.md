# Part 12 (hook) — closing bd discr-ovl.1: the two anticipation cascades

**bd discr-ovl.1 CLOSED.** Both hit tests' anticipation cascades are decoded
end to end, all four `disc+$12` hook installs are implemented in `disc-core`
itself (`crate::player::anticipate`, called from both `hit_test` and
`p2_hit_test`, wired into `disc::step`), and `tracecheck` no longer feeds
`discs[n].hook` — it is a compared field like `state_index`, never in the
`WAIVED` list. This report consolidates and closes out work that was largely
already recorded piecemeal across Parts 10f, 11, 11b and 11j of
`docs/disc-notes.md`; it adds the live measurement of `p1_walk` frame 272
(the bead's known first consumer) and fixes several stale `UNKNOWN:
discr-ovl.1` doc comments left over from before the cascade was implemented.

## The four hooks

| hook | ST addr | aim | axes | installed by |
|---|---|---|---|---|
| `$a71a` | player 1 deep | `$6ca2 - $13` | X, then `$a758`'s Y | `$113e2` (intercept) |
| `$a78e` | player 1 shallow | `$6ca2 - $04` | X only | `$11334` (track start), `$11372` (reach) |
| `$a7d8` | player 2 shallow | `$6d22 - $04` | X only | `$cb70` (track start), `$cbae` (reach) |
| `$a816` | player 2 deep | `$6d22 - $13` | X, then `$a854`'s Y | `$cc1e` (intercept) |

Each player's cascade installs its **shallow** hook the moment it starts
tracking a disc, and its **deep** hook only if it commits to stepping across
to intercept. `docs/disc-notes.md:2012` (Part 11) is where the fourth hook,
`$a78e`, was found — three hooks had been assumed complete since Part 10f, and
a fixture minted specifically to look for a swing found it installed instead.

## Player 2's cascade — `$cb2c`-`$cc9a` (`docs/disc-notes.md:1550`, Part 10f)

```
$cb2c  tst.b $6d2e ; bne out            ; only from state 0 (idle)
$cb34  cmpi.b #$7,$6d29 ; beq out       ; and facing != 7 (racket)
$cb3e  tst.w ($0a,a5) ; bmi/beq out     ; dir_kind > 0 -- travelling AWAY
$cb4a  tst.b ($11,a5) ; bne out         ; owner byte == 0
$cb52  d5 = $6d26 - $6d32               ; depth - reach ...
$cb56  ...or minus $32 under bonus code 5
$cb6a  if d2 < d5 -> out                ; exit if shallower
$cb70  move.l #$a7d8,($12,a5)           ; INSTALL $a7d8 -- start tracking
$cb78  d5 += reach ; d5 -= $c ; if d2 < d5 -> out
$cb96  d5 += 2      ; if d2 > d5 -> out ; a two-unit-deep narrow window
$cb9e  d5 = $6d22 - 3                   ; the X ladder pivot
       within $c right    -> REACH ($cbae)
       $f..$22 further right -> probe cell +$c, can_stand? step : reach
       further still         -> out
       on the pivot          -> REACH
       within $f left     -> REACH
       $22 further left   -> probe cell -$c, can_stand? step : reach
       further still         -> out
$cbae  keep $a7d8, animation $466a, state $1b = 27 (REACH)
$cc1e  install $a816, animation $4612, state $12 = 18 (INTERCEPT)
```

`can_stand` (the step-across probe) tests the probed cell against the row
threshold `$6d26 > $3a` (58, player 2's own `world_y` of 54) and against its
own bank `$7596`. `$cb70` fires whether or not either terminal state is
entered — 28 times against one each of `$cbae`/`$cc1e` in
`tile_damage.ndjson` — because tracking a disc and committing to a response
are separate decisions.

## Player 1's cascade — `$112f4`-`$1147a` (`docs/disc-notes.md:2443`, Part 11j)

Tail of `$10fd8`, entered from every non-crossing and every body-box miss
(`$10fee`, `$10ff6`, `$11000`, `$11008`, `$11108`, `$11114`, `$11122`). Same
code as `$cb2c` with three constants swapped and, per Part 11j, one crucial
extra gate:

| | player 2 (`$cb2c`) | player 1 (`$112f4`) |
|---|---|---|
| idle + `facing != 7` | `$cb2c`, `$cb34` | `$112f4`, `$112fc` — identical |
| disc travelling away | `dir_kind > 0` (`bmi`/`beq` exit) | `dir_kind < 0` (`bpl` exit) |
| owner byte | `== 0` (`bne` exits) | `!= 0` (`beq` exits) |
| near edge | `depth - reach` (`$cb52`) | `depth + reach` (`$11316`) |
| bonus-5 arm | `$6d9a`, `- $32` | `$6d1c`, `+ $32` |
| exit when | shallower (`$cb6a`) | deeper (`$11330`) |
| shallow hook | `$a7d8` | `$a78e` |
| deep hook | `$a816` | `$a71a` |
| narrow window | `[depth-$c, depth-$a]` | `[depth+$a, depth+$c]` |
| X ladder | `$c`/`$18` right, `$f`/`$22` left, `$c` probe | identical |
| reach anim | `$466a`, state `$1b` | `$2c56`, state `$1b` |
| intercept anim | `$4612`, state `$12`, hook `$a816` | `$2bfe`, state `$12`, hook `$a71a` |
| `can_stand` row threshold | `$6d26` vs `$3a` (58) | `$6ca6` vs `$e` (14) |
| own bank | `$7596` | `$7616` |
| reach constant | `$6d32` (26) | `$6cb2` (12) |

Three sign flips (depth axis, owner byte, exit direction), one different
bonus word, four different addresses — and the seven X-ladder constants and
the whole reach/intercept animation shape are identical, because X is the
same direction for both players and depth is not.

**The real blocker was not the cascade.** Its third gate reads the disc's
owner byte (`disc+$11`), and until Part 11j fed that field (now settled by
`discr-ovl.2`, `docs/disc-notes.md:2479`), it stayed frozen at the frame-0
seed and the gate rejected every disc for the whole replay. Transcribing the
cascade correctly changed nothing until that field moved.

## Implementation — `disc-core` installs its own hooks

`crate::player::anticipate` (`crates/disc-core/src/player.rs:1040`)
implements both halves as one function parameterised on `PlayerId`, mirroring
the table above exactly (gates, narrow window, X ladder, `can_stand` probe).
It is called from `hit_test` (player 1, `crates/disc-core/src/player.rs:635`)
and `p2_hit_test` (player 2, `:825`), both of which are called from
`disc::step` (`crates/disc-core/src/disc.rs:531-556`) at exactly `$a652` and
`$a656` — inline, in the same per-tick pass that clears hooks at the wall
bounds, matching the ST's own ordering (bounds clear mid-frame, hit tests
reinstall at the end).

Several doc comments predating this implementation still claimed the install
was `UNKNOWN`/fed — stale since commits `066e997` (hook install landed) and
`19ac647` (player 1's half completed all three fixtures). Corrected in this
pass: `crates/disc-core/src/disc.rs` (module doc's "what this module still
does not decide" list, the `step` doc's "two ST behaviours not reproduced"
bullet, `AIM_X_WIDE_OFFSET`'s doc), `crates/disc-core/src/player.rs`
(`hit_test`'s doc, which had incorrectly attributed the hook install to the
still-unmodelled racket path, states 7..10 — the racket path does not, on the
evidence, install a hook at all; that remains a separate open gap, tracked
under `discr-b6x` alongside player 2's AI policy, not `discr-ovl.1`), and
`crates/disc-tools/src/main.rs` (the `hook` field's parse comment in
`Frame::to_state`, which still called it a feed).

## Cash-in — `tracecheck` no longer feeds `disc+$12`

`discs[n].hook` is absent from `main.rs`'s `WAIVED` list (`const WAIVED: [(&str,
&str); 1] = [("players[1].", "discr-b6x")]`), so it is unconditionally
compared every tick regardless of `--skip-waived`; `feed_disc_inputs` does not
touch it (its own comment, `crates/disc-tools/src/main.rs:622`, already
recorded this: *"`disc+$12`, the steering hook, came off this list in Part
10f"*). `resync`'s `skip("discs[{n}].hook")` arm exists only for the general
`--resync <FIELD>` escape hatch used to localise divergences during
development, not as a standing waiver. This is the "compare it instead of
feeding it" cash-in the bead asked for, and it was already in place before
this pass — confirmed here, not newly built.

## p1_walk frame 272 — the bead's known first consumer, measured live

```
$ cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/p1_walk.ndjson \
      --dump discs[0].hook --from 269
  tick 269 in  discs[0].hook=0/0
  tick 270 in  discs[0].hook=0/0
  tick 271 in  discs[0].hook=0/0
  tick 272 in  discs[0].hook=42778/42778      # 0xa71a, expected == got
  tick 273 in  discs[0].hook=42778/42778

$ cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/p1_walk.ndjson \
      --dump players[0].state_index --from 269
  tick 272 in  players[0].state_index=18/18   # $12, expected == got
```

`players[0].facing` moves to 18 on the same tick. All three match exactly:
`disc-core`'s own `anticipate` installs `$a71a`, enters the intercept
sequence and sets state 18 on the identical frame the ST does. `p1_walk`
reproduces 274 of 274 ticks, nothing waived — unchanged from before this
pass, because the implementation was already correct; what this pass adds is
the direct measurement tying it to the bead.

## Gates (this worktree, unchanged numbers)

| gate | result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo test` | 45 + 11 passed |
| `golden.ndjson --skip-waived --min-agree 99` | 99/99 |
| `tile_damage.ndjson --skip-waived --min-agree 214` | 214/214 |
| `golden.ndjson --min-agree 99` | 99/99 |
| `tile_damage.ndjson --min-agree 214` | 214/214 |
| `p1_walk.ndjson --min-agree 274` | 274/274 |
| `handover.ndjson --min-agree 21` | 21/21 |
| `handover.ndjson --skip-waived --min-agree 222` | 222/222 |

No gate moved. This bead's work was documentation, doc-comment correction,
and confirmation of already-landed code — not new cascade logic.

## A note on fixture provenance in this worktree

`tests/fixtures/handover.ndjson`/`.provenance.md` were minted by a sibling
agent (`owner`, bd `discr-ovl.2`, commit `2e22021`) on the fleet's shared
integration branch (`claude/atari-abandonware-download-muie9c`) while this
worktree's branch was still forked from before that wave landed, and a
worktree-isolated agent cannot merge or rebase onto the shared branch itself
(`git merge`/`git rebase` are refused; `pact merge` runs only from the main
checkout — see `AGENTS.md`'s worktree-fleet note). They were pulled in
read-only via `git show 2e22021:<path>`, verified byte-identical
(`sha256sum` matches the blob), so the two required `handover.ndjson` gates
above could run. Nothing in that fixture or its provenance was re-decided
here; the polarity finding it supports (`discr-ovl.2`, disc+$11) is a
separate field from this bead's `disc+$12` and is out of scope for
`discr-ovl.1`.

## Out of scope, left for their own beads

* **The racket path, states 7..10** (`$11030`-`$11096`): swinging, not
  anticipating. No install tied to it on the evidence. `discr-b6x`.
* **Disc retirement** (what takes `disc+$10` off `$ff` outside the caught-disc
  countdown already modelled): `discr-0fm`.
* **`disc+$11`'s owner-polarity / `PlayerId` convention mismatch**: settled
  factually (`discr-ovl.2`, CLOSED) but the coordinated three-file rename
  fixing the backwards-from-real-player internal convention is filed
  separately (`discr-ovl.8`) and deliberately not touched here.

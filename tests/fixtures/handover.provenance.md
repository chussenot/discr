# `handover.ndjson` — provenance

450 frames. The fourth committed fixture, minted in Part 12 (discr-ovl.2) to
catch the disc's owner byte (`disc+$11`) actually changing hands, and to name
which of `$6d8a`/`$6d8c`/`$6d0a`/`$6d0c` moves which way when it does.

    printf 'j 5 04 00\nj 30 00 00\n' > tmp/handover.script
    ./oracle/disc-oracle --seed seeds/match_challenge.seed \
        --script tmp/handover.script --frames 450 \
        --trace tests/fixtures/handover.ndjson

* **Seed**: `seeds/match_challenge.seed`, sha256
  `0b23f4d6583b5c80386adf5ef210dd84728775c39bfee8f5bc9bb98f003779f9`, captured
  2026-08-30 at `PC == $8198` in a fresh live CHALLENGE round (`$6ab4` = 6331),
  via `scenarios/oracle_seed.yaml` against
  `Disc (1990)(Loriciel)[cr Exo-7].st`. Gitignored; the manifest entry lives
  here rather than in `seeds/MANIFEST.md` because that file is gitignored too
  (see `.gitignore`).
* **Input**: identical programme to `p1_walk.ndjson` — player 1 walks **left**
  from frame 5 to frame 30 and then stands still for the rest of the window.
  Player 2 is the AI throughout. The point of reusing the exact programme is
  that it is the one already known (from `p1_walk`) to let a disc go uncaught
  long enough to reach a real wall bound; this seed simply runs it 175 frames
  longer (450 vs 275) and happens to land the disc in a different starting
  slot, which is what catches the **return** leg `p1_walk` never reached.
* **Independent Hatari validation**: NOT done. Like `p1_walk`, this trace's
  authority is that the oracle executes the real disassembled 68000 code
  (Musashi against the actual ROM/game image), not a Hatari differential run
  against this specific programme. Numbers measured against it are
  `disc-core` against the oracle, not against the machine.

## Why it exists

`docs/disc-notes.md` ("The disc update loop `$a4ea`, in full", Part 10) named
the wall handlers that flip `disc+$11` and move the four counters, but every
trace on hand at the time read owner `0` on every live slot. Part 11j found
the first exception (`p1_walk`, one flip, one direction, frame 220). This
fixture was minted to get the **other** direction too, in the same trace, so
the counter symmetry could be checked against real data rather than asserted
from the disassembly alone.

## What it shows

Disc slot 1's owner byte, across the run (`own` column, disc index 1):

```
frame   0- 52   own = None (not yet live)
frame  53-258   own = 0
frame 259-338   own = 255
frame 339-449   own = 0
```

Two flips, opposite directions, same slot:

* **Frame 259 — the FAR wall (`world_z` crossing 79), owner 0 -> 255.**
  `players[0].discs_out` 0->1 and `players[0].disc_cap` 0->1 in the SAME
  frame; `players[1].discs_out` 3->2 and `players[1].disc_cap` 4->3.
* **Frame 339 — the NEAR wall (`world_z` crossing 0), owner 255 -> 0.**
  `players[0].discs_out` 1->0 and `players[0].disc_cap` 2->1;
  `players[1].discs_out` 0->1 and `players[1].disc_cap` 2->3.

`players[n].discs_out`/`disc_cap` are already-modelled fields (ST `player+$6a`/
`+$6c` region via the trace's own columns) that mirror `$6d8a`/`$6d8c` for
player 2 (`players[1]`, confirmed: the intercept handler `$c196` that reads
`$6d8a`/`$6d8c` is textually player 2's own state-18 code) and `$6d0a`/`$6d0c`
for player 1 (`players[0]`, by elimination and by the symmetric ST
disassembly — see `reports/part12-owner.md` for the full chain). The two
frames above are the trace's own recorded columns; disc-core's own replay
does not need to reach them for this to be evidence, and it currently does
not (see "Validation" below).

Full corroborating detail, including static Ghidra disassembly at every PC
named, `reports/part12-owner.md`.

## Validation: NOT a long clean run, on purpose

Unlike the other three fixtures, this one is not being offered as a new
reproduction-length record. `disc-core`'s replay of it diverges early --
tick 21 without `--skip-waived`, tick 222 with it -- on
`discs[0].active` (ST moves 255 -> 2 across one tick; disc-core's
single-step `wrapping_add` retirement countdown lands on 1). That is an
existing gap in the active-byte retirement model (a disc caught AND
counted down in the same tick, i.e. another multi-update-pass tick in the
`$96b6`/`$96ba` sense Part 11f/11g already named for `p1_walk`, just hitting
a different field this time) and is not something this bead touches. The
value of this fixture is in its own recorded columns at frames 259 and 339,
not in how far `disc-core` gets before diverging on an unrelated field.
`mise.toml`'s `HANDOVER_MIN_AGREE`/`HANDOVER_SKIP_MIN_AGREE` gate exactly
those two measured numbers (21, 222) so a future regression is still caught,
without implying the fixture is meant to grow into a fifth "whole fixture
reproduces" record.

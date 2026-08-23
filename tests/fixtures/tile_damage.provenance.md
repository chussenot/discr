# tile_damage.ndjson — provenance

215 frames. The fixture the project lacked: one where a disc actually damages
and destroys tiles, so `tile::damage` is exercised by trace comparison instead
of only by unit tests.

    ./oracle/disc-oracle --seed seeds/diff.seed \
        --script tmp/idle.script --frames 215 --trace <out>

* **Seed**: `seeds/diff.seed`, sha256 `4dee348993853659...`, captured at
  PC == `$8198` in a live CHALLENGE round (`$6ab4` = 6949).
* **Input**: none. Idle. The tile events come from the opponent's play, which
  is why no input programme was needed to provoke them.
* **Why the window is trustworthy**: `scripts/oracle_diff.py` reports **275
  frames of tier-1 (frame-exact) agreement** with Hatari over this seed. All
  215 frames here are inside it.

## The tile events it contains

Oracle frame numbers; the Hatari reference sits 5 frames earlier because
minting the seed costs a `savebin`.

| oracle frame | cell | change | reading |
|---|---|---|---|
| 70 | 6 | `(1,1) -> (0,0)` | destroyed: hp reached 0 and `$a354` cleared the type |
| 119 | 14 | `(1,1) -> (0,1)` | **anomaly** — type cleared, hp untouched; not `$a354`. bd discr-b4q |
| 170 | 7 | `(2,4) -> (2,1)` | damaged, -3 |
| 208 | 8 | `(1,4) -> (1,1)` | damaged, -3 |

The frame-119 row is the interesting one and is why this fixture is worth
keeping even though `disc::step` cannot yet reach `tile::damage`
(bd discr-5w5): it is a tile change that the decoded damage rule **cannot**
produce, sitting inside a validated window.

## Coverage note

Recording this fixture required widening the Hatari memdump window. At
`nMemdumpLines = 200` it stopped at `$767f`, so tile cells **13-16 were never
compared** — the far row of the floor, where the player stands. `hatari.cfg`
now uses 232 lines and `scripts/oracle_diff.py` asserts full coverage instead
of skipping absent bytes.

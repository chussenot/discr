# Seed provenance

Seed binaries are derived from the disk image and are gitignored; this file records where each one came from so a result can be traced back to a frame-exact origin.

A relayed seed is only listed once `scripts/seed_verify.sh` has confirmed it: both emulators identical at frame 0 and agreeing for a short differential run.

| seed | sha256 | $6ab4 | parent | programme | frame | parent validated to |
|---|---|---|---|---|---|---|
| `rally_f100.seed` | `f256ebf6a1eba811` | 7058 | `diff.seed` | rightfire | 100 | 116 |
| ~~`quiet_f100.seed`~~ REJECTED | `2f39c96e789b741f` | 7053 | `diff.seed` | rightpause | 100 | 109 |

## Rejected seeds

`quiet_f100.seed` (rightpause frame 100) did **not** verify and must not be used. Hatari dropped a frame inside the ~5-frame gap between minting and the start of the reference trace -- the gap is the cost of the 1 MB `savebin` and is not under our control -- so the two sides start one frame apart. Oracle frame 4 matches Hatari trace 0 to within 5 bytes (the per-frame counters) while frame 5 differs in 70; per the tier-2 finding, once they are off by one they part.

Its **Hatari reference is still ground truth** and is used as such: states 11 and 19 were read from it directly. What is unusable is the seed as an oracle starting point.
| `rally_f100.seed` | `bb3cf70118694674` | 7057 | `diff.seed` | rightfire | 100 | 116 |

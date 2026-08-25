# Disc (Loriciel, 1990, Atari ST) — reverse engineering

Recovering the game rules of *Disc* from the original 68000 code, with every
claim tied to an address and a measurement. Three things live here:

| | |
|---|---|
| `oracle/` | `disc-oracle` — the game code running headless under Musashi, differentially validated against Hatari. `oracle/README.md`. |
| `crates/disc-core` | The rules, re-implemented in Rust from `docs/disc-notes.md`. No dependencies, integer arithmetic only. |
| `crates/disc-tools` | `tracecheck` — replays an ST trace against `disc-core` and reports the first divergence. |

Read `reports/` before the code: `exploration-report.md` (what the game does),
`oracle-report.md` (how far the oracle is trusted), `core-report.md` (how far
`disc-core` reproduces the ST, and what is waived). `docs/state-schema.md` is
the field-by-field contract between `disc-core` and a trace, and
`KNOWN_ISSUES.md` is the standing list of things that bite.

## The two gates

    mise run oracle-check    # oracle vs Hatari, differential
    mise run core-check      # fmt + clippy + tests + four tracecheck runs
    cargo run --release      # a bare tracecheck over the golden fixture

Both are green today, and **all four of `core-check`'s runs are clean**:

| run | result |
|---|---|
| `golden --skip-waived` | 99 of 99 ticks, no divergence |
| `tile_damage --skip-waived` | 214 of 214 |
| `golden`, nothing waived at all | 99 of 99, both players |
| `tile_damage`, nothing waived | 214 of 214, both players |
| `p1_walk`, nothing waived | 255 of 274 — the one gate that is not clean, on purpose |

They still gate on a **measured prefix** via `--min-agree` rather than on zero
divergence, because that is what catches a regression: a clean run can only get
shorter, and the number says by how much. `reports/part10-report.md` has the
history of every one of those numbers; `reports/core-report.md` is the older
account and says at the top what Part 10 superseded.

Clean does not mean finished. `disc-core` reproduces everything these two traces
do — 313 ticks of it — and seven ST fields are still fed each tick: six of them
animation-derived data or per-player constants, plus `updates`, the main-loop
pass count Part 11f added (the game update lives in the main loop at `$96ba`,
not the VBL, so a sampled frame contains 0, 1 or 2 passes); the run's own
header names them.
`docs/state-schema.md` has the seventeen waived rows and the beads that own them.

`oracle-check` is prefix-gated for a different and permanent reason: the oracle
and Hatari agree byte-for-byte for a bounded window, and
`reports/oracle-report.md` says why.

## What you need installed

The emulator inputs are gitignored and never committed. Fetch both into the
(equally gitignored) `tmp/` directory with:

    ./scripts/fetch_assets.sh

which downloads:

* `tmp/Disc (1990)(Loriciel)[cr Exo-7].st` — the game disk image, from its
  abandonware page (https://www.myabandonware.com/game/disc-vp). If the
  scrape breaks, the script says how to finish by hand (`DISC_URL=<direct
  link>`, or drop the zip into `tmp/` and re-run).
* `tmp/emutos-512k-1.4/etos512us.img` — the TOS ROM: EmuTOS 1.4 (GPL;
  source at https://github.com/emutos/emutos), from its [SourceForge
  releases](https://sourceforge.net/projects/emutos/files/emutos/1.4/),
  where the source package and the other binary packages also live.

`scripts/collect.py` looks for both under `tmp/` first, then at the repo
root (their old home). It needs `curl` (or `wget`) and `unzip`.

## Versioning

The Rust workspace is a **cocogitto monorepo**: `crates/disc-core`,
`crates/disc-tools` and `crates/disc-app` each carry their own tag and
changelog, and `cog bump --auto` moves only the packages whose files changed.

    mise run check-commits    # lint commit messages
    mise run changelog        # preview, writes nothing
    cog bump --auto           # per-package + global tags (yours to run)

Only `disc-core` is `public_api = true`: it is the library the other two build
against, so its breaking changes drive the global major. `disc-tools` and
`disc-app` are binaries whose interface is a command line and a window, so
their breakage is theirs alone.

Everything outside `crates/` — `oracle/`, `scripts/`, `docs/`, `reports/` — is
reverse-engineering apparatus rather than a published artifact, and rides the
global tag.

Two commit types are registered beyond the conventional set, because this
project does both often enough to want them in a changelog: **`measure`** (a
number established by experiment) and **`retract`** (an earlier claim withdrawn
on new evidence).

The repository's first three commits predate the convention, so the lint runs
from `$CONVENTIONAL_SINCE` in `mise.toml` rather than ignoring a rule.

**`mise run core-check` needs a Rust toolchain and nothing else.** No system
libraries, no network, no emulator, no disk image — it runs from a clean clone
because `tests/fixtures/golden.ndjson` is committed (with `git add -f`; the
`*.ndjson` rule in `.gitignore` would otherwise catch it). `mise.toml` pins the
toolchain; `mise install` is enough.

`mise run oracle-check` needs more, and cannot run from a clean clone at all:

* a C compiler, `make`, and **OpenSSL headers** — `oracle/Makefile` links
  `-lcrypto` for the per-frame state hash (`apt install libssl-dev`, or
  `openssl` from your package manager);
* Python 3 and **Hatari** with a TOS ROM, to record or refresh the reference;
* `seeds/` and the disk image, which are **gitignored and never committed** —
  the whole directory is, so it is absent from a fresh clone. That is
  deliberate: the fixtures are committed precisely because their inputs are
  not. (`scripts/regen_golden.sh` and `tests/fixtures/golden.provenance.md`
  both point at a `seeds/MANIFEST.md` that is gitignored along with everything
  else under `seeds/`, so a fresh clone cannot read it.)

`crates/disc-app` (the playable macroquad front end) is deliberately **outside
the workspace `default-members`** so its system dependencies — `libGL`,
`libX11`, `libasound` — can never break `mise run core-check`. Build it explicitly:

    cargo run -p disc-app

## Working here

`AGENTS.md` is the coordination protocol (pact) and the issue tracker (bd)
contract. `docs/disc-notes.md` is the evidence base: implement from a citation
in it, never from memory, and if a note and the code disagree the note wins
until someone re-measures.

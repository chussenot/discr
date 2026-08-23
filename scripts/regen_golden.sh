#!/bin/sh
# Regenerate tests/fixtures/golden.ndjson, the tracecheck golden fixture.
#
# Bead discr-3g6. Provenance lives in tests/fixtures/golden.provenance.md;
# this script is the executable half of it.
#
# REQUIRES seeds/diff.seed, which is GITIGNORED (see seeds/MANIFEST.md and the
# `seeds/` entry in .gitignore). The seed is derived from the disk image and is
# never committed, so this script cannot run on a fresh clone. That is the
# point: the fixture is committed precisely because its input is not.
#
# The fixture itself is also matched by the `*.ndjson` gitignore rule and was
# committed with `git add -f`. Do the same if you regenerate it.
set -eu

cd "$(dirname "$0")/.."

SEED=${SEED:-seeds/diff.seed}
SCRIPT=${SCRIPT:-tmp/leftright.script}
FRAMES=${FRAMES:-100}
OUT=${OUT:-tests/fixtures/golden.ndjson}

for f in ./oracle/disc-oracle "$SEED"; do
    [ -e "$f" ] || { echo "regen_golden: missing $f" >&2; exit 1; }
done

# The leftright programme: Left from frame 9 to 27, Right from frame 102.
# Written out here so the fixture does not depend on a scratch file surviving.
if [ ! -e "$SCRIPT" ]; then
    mkdir -p "$(dirname "$SCRIPT")"
    printf 'j 9 04 00\nj 27 00 00\nj 102 08 00\n' > "$SCRIPT"
fi

./oracle/disc-oracle --seed "$SEED" --script "$SCRIPT" --frames "$FRAMES" --trace "$OUT"

echo "regen_golden: wrote $OUT ($(wc -l < "$OUT") frames)"
echo "regen_golden: verify with  cargo run -p disc-tools --bin tracecheck -- $OUT"
echo "regen_golden: re-add with  git add -f $OUT   (*.ndjson is gitignored)"

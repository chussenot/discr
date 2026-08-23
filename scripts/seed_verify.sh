#!/bin/sh
# A relayed seed is not trusted until both emulators agree from it.
#   seed_verify.sh <seed> <hatari-reference-cache> [min-frames]
set -e
cd "$(dirname "$0")/.."
SEED=$1; CACHE=$2; MIN=${3:-30}
[ -n "$SEED" ] && [ -n "$CACHE" ] || { echo "usage: seed_verify.sh <seed> <cache> [min]"; exit 2; }
make -s -C oracle
printf '# idle\nj 0 00 00\n' > tmp/verify_idle.script
exec ./scripts/oracle_diff.py --seed "$SEED" --cache "$CACHE" \
     --script tmp/verify_idle.script --frames 140 --min-agree "$MIN"

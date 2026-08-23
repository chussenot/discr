#!/bin/sh
# Differential suite + determinism double-run.  Uses the cached Hatari
# reference if one exists; pass --refresh to re-record it.
set -e
cd "$(dirname "$0")/.."
make -s -C oracle

SEED=${SEED:-seeds/diff.seed}
FRAMES=${FRAMES:-400}

echo "== determinism: same seed + script twice must be byte-identical"
for i in 1 2; do
  ./oracle/disc-oracle --seed "$SEED" --script tmp/idle.script \
      --frames "$FRAMES" --window 0x6a00 0x76c0 --trace "tmp/det$i.ndjson"
done
if cmp -s tmp/det1.ndjson tmp/det2.ndjson; then
  echo "   OK: two runs identical ($(wc -l < tmp/det1.ndjson) frames)"
else
  echo "   FAIL: runs differ"; exit 1
fi

echo "== differential validation against Hatari (idle)"
./scripts/oracle_diff.py --frames "$FRAMES" --min-agree 275 "$@"

echo
echo "== differential validation against Hatari (joystick sweep)"
./scripts/oracle_diff.py --frames "$FRAMES" --input sweep --min-agree 360 "$@"

echo
echo "== differential validation against Hatari (right+fire: disc serve path)"
./scripts/oracle_diff.py --frames "$FRAMES" --input rightfire --min-agree 112 "$@"

if [ -f seeds/rally_f100.seed ] && [ -f tmp/hatari_ref_rally_f100.json ]; then
  echo
  echo "== relayed seed (rally_f100)"
  exec ./scripts/seed_verify.sh seeds/rally_f100.seed tmp/hatari_ref_rally_f100.json 30
fi
echo
echo "(relayed seed absent -- mint with scripts/seed_relay.py to include it)"

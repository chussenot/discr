#!/bin/sh
# Query the analysed image.  Commands, which compose in one invocation:
#
#   xref <hex>       every reference to that address
#   scan <hex>       every disassembled instruction with that operand -- finds
#                    addresses used as DATA, e.g. move.l #$a7d8,($12,a5),
#                    which the reference model files elsewhere
#   dec  <hex>       decompiled C of the containing function
#   dis  <hex> <n>   n instructions from there
#   fun              every function found
#
#   scripts/ghidra/q.sh dis a4ea 60 xref 6c59 scan a816
set -e
. "$(dirname "$0")/env.sh"
"$HEADLESS" "$GHIDRA_PROJ" disc -process discram.bin -noanalysis -readOnly \
    -scriptPath "$SCRIPTS" -postScript Q.java "$@" 2>&1 \
  | sed -n 's/^INFO  Q\.java> //p' | sed 's/ (GhidraScript)  *$//'

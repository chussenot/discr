#!/bin/sh
# One-off: import a 1 MB Atari ST RAM image as a raw 68000 binary at address 0
# and auto-analyse it.  SeedEntries.java runs first and disassembles the entry
# points docs/disc-notes.md already knows, which is what gives auto-analysis
# something to follow -- a raw image has no symbols and no entry point, so
# without the seed nothing at all gets disassembled.
#
# Takes about 90 s.  Re-run only to switch images.
set -e
. "$(dirname "$0")/env.sh"
mkdir -p "$GHIDRA_PROJ"
cp -f "$GHIDRA_IMAGE" "$GHIDRA_PROJ/discram.bin"
"$HEADLESS" "$GHIDRA_PROJ" disc \
    -import "$GHIDRA_PROJ/discram.bin" \
    -processor 68000:BE:32:default \
    -loader BinaryLoader -loader-baseAddr 0x0 \
    -scriptPath "$SCRIPTS" -preScript SeedEntries.java \
    -overwrite

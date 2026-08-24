# disc-oracle

Runs the Disc (Loriciel, 1990) game code under [Musashi] as a headless,
deterministic 68000 trace generator. Hatari remains the reference; this
exists so traces are cheap enough for a test suite.

    mise run oracle        # or: make -C oracle
    ./disc-oracle --seed ../seeds/match_challenge.seed \
                  --script script.txt --frames 300 \
                  --window 0x6a00 0x76c0 --trace out.ndjson

## Vendored Musashi

    https://github.com/kstenerud/Musashi
    commit 313ebf1bd9f4d0d93341eb5ce21fd8a119e9dbdd

Local changes are confined to `musashi/m68kconf.h`: the 010/020/040 cores are
switched off (the game is a plain 68000 ST, proven in Phase 0), and the
int-ack callback and instruction hook are pointed at `disc_int_ack` and
`disc_instr_hook`.

## The machine it emulates

Exactly what `reports/oracle-scope.md` measured the game to need:

* 1 MB of RAM as raw big-endian bytes — the same layout Hatari's `savebin`
  writes, so hashes and byte ranges are comparable by construction.
* Level-4 VBL once per frame, vectoring to `$8198`.
* An IKBD ACIA at `$FFFC00`/`$FFFC02` that emits joystick packets **on state
  change only** (`$FF`,joy1 / `$FE`,joy0) and raw key scancodes.
* Write-only stubs for the PSG (`$FF8800`-`$FF8807`), palette
  (`$FF8240`-`$FF825F`) and screen base (`$FF8201`/`$FF8203`), and read/write
  storage for the four MFP timer registers.

Anything else in `$FF8000`-`$FFFFFF` **aborts the run**, printing the address
and PC. `--permissive` downgrades that to a logged warning; a permissive run
that "passes" is still auditable because every forgiven access is on stderr.

Timer A and Timer B are deliberately **not** emulated. Phase 0 established
that Timer A is a PSG streamer (its only RAM effect is advancing USP) and
Timer B writes one palette register, so neither can touch state below
`$8000`. If the differ ever disagrees, that assumption is the first suspect.

## The sampling-point contract

    state(N) = memory as it stands at entry to the VBL handler for frame N,
               with PC == $8198, BEFORE that instruction executes.

This is not observable from outside `m68k_execute()`: the interrupt dispatch
and the handler's first instruction happen inside one call, so an external
PC test always sees `$819c` and samples a frame late. The frame is therefore
emitted from inside Musashi's instruction hook, which does fire
pre-execution.

## Script format

    # comment
    j <frame> <joy1 hex> <joy0 hex>    joystick state at the start of <frame>
    k <frame> <scancode> <0|1>         key break(0) / make(1)

Joystick bits: `$01` up, `$02` down, `$04` left, `$08` right, `$80` fire.

[Musashi]: https://github.com/kstenerud/Musashi

## Investigation flags (Part 10)

Two flags exist for reverse engineering rather than for validation, and both
answer questions that used to cost a Hatari debugger session each.

    --watch LO HI     report every write into [LO, HI) with the PC that made it
    --disasm ADDR N   disassemble N instructions from ADDR in the seeded RAM

`--watch` is the more useful of the two. "Who writes this address" is the
question this project asks most often, and one run answers it for a whole range
at once:

    ./oracle/disc-oracle --seed seeds/diff.seed --frames 215 \
        --trace /dev/null --watch 0x7596 0x7696

    watch frame  69  pc $00a34c  write.w $007648 = $0000
    watch frame  69  pc $00a354  write.w $007646 = $0000
    watch frame 118  pc $014bb8  write.w $007686 = $0000
    watch frame 169  pc $00a34c  write.w $007650 = $0001
    watch frame 207  pc $00a34c  write.w $007658 = $0001

Five writes into both tile banks across 215 frames, and the one at `$14bb8` is
the writer bd discr-b4q had been open on since Part 8. Reports go to stderr, so
`--trace` still works alongside; the report cap is 4000 lines.

`--disasm` runs Musashi's own disassembler over the seeded image and exits
without simulating. It is for addresses `scripts/ghidra/` has not reached --
a raw 1 MB image is mostly unreachable from any set of seeded entry points, and
re-importing to read twenty instructions costs ninety seconds.

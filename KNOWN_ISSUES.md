# Known issues and corrections

Everything here was found empirically while building the pipeline; each entry
is something that did **not** behave the way the task brief or the Hatari docs
suggested, plus what the code does about it.

## Corrections to the starting assumptions

**`ramdiff.py` lives in `scripts/`, not the repo root**, and its CLI is
positional triplets (`OP fileA fileB ...`), not `changed(a,b)` expressions.
`scripts/analyze.py` calls it in that form.

**`hatari.cfg` did not exist**, and `~/.config/hatari/hatari.cfg` had
`[Joystick1] nJoystickMode = 1`. That is **not** keyboard emulation --
Hatari's enum is `0 = none, 1 = real stick, 2 = keys`. With the stock config
the cursor keys and Right Ctrl were delivered to the emulated machine as plain
ST *keyboard* scancodes and the game (which polls the joystick) never saw
them. The repo now ships its own `hatari.cfg` and additionally passes
`--joy1 keys` on the command line.

**The frame counter is at `$6ab4`, not `$6ab6`.** `$6ab6` is zero in every
dump taken in a match; `$6ab4` increments by exactly 1 per VBL and never
reverses (see the `disc_flight` table in `reports/findings.md`).

**The title screen needs SPACE, and the menu is joystick-driven, not
mouse-driven.** The arrow drawn on the menu is moved by joystick 1, and
joystick fire selects. There is no working mouse path at all (see below), so
no click coordinates were needed.

## Hatari 2.6.1 quirks the collector works around

**The debugger's `echo` command aborts the emulator.**
`hatari-debug echo anything` dies with
`hatari: ./src/str.c:240: Str_UnEscape: Assertion 's2 < s1' failed.`
Command replies are therefore bracketed with `evaluate #<n>` markers instead,
whose output (`#<n> (dec)`) is just as easy to find in the log.

**Debugger output is split across two streams with different buffering.**
Command output (`memdump`, `cpureg`, `disasm`, `evaluate`) goes to stdout,
while the `> cmd` echo and everything else goes to stderr. Piped, stdout is
block-buffered, so the merged log arrives out of order and reply markers show
up late or after the next command. Hatari is launched under
`stdbuf -oL -eL` to line-buffer both.

**Long debugger output pages, and the pager eats the next input line.**
A `disasm` over a wide range stops after N lines and swallows the following
socket command -- which was the reply marker, so the capture timed out. Two
mitigations: the shipped `hatari.cfg` raises `nDisasmLines`/`nMemdumpLines` to
200, and `dbg_capture()` re-sends its end marker once at half the timeout.

**`statesave` prompts before overwriting.** With stdin on `/dev/null` the
prompt is answered with EOF and the save is silently cancelled, while the
command appears to hang. `statesave()` unlinks the target first.

**The control socket has no mouse-motion event.** `hatari-event` supports only
`doubleclick`, `rightdown`, `rightup`, `keypress`, `keydown`, `keyup`, and its
keys are injected as ST scancodes straight into the IKBD, bypassing Hatari's
SDL-level joystick emulation entirely. Joystick input therefore has to be real
X key events, which is why the collector runs Hatari on an Xvfb display and
injects with XTEST. That is also what makes the pipeline headless --
`SDL_VIDEODRIVER=dummy` was not needed or tried, since Xvfb already works and
keeps XTEST available.

**Injected mouse input never reached the emulated machine.** XTEST pointer
warps and button presses do reach Hatari (`F11` toggles fullscreen, and a
button press shows up as an IKBD joystick-0 fire packet), but the ST mouse
pointer never moved, with or without `hatari-shortcut mousegrab`. Not chased
further, because the game turned out not to need the mouse. If a future
scenario does, this is the open problem.

**The game writes to the floppy image.** Without `--protect-floppy on` Hatari
logs `Updated the contents of floppy image ...` on exit. The collector always
passes it; do not remove it.

## Game behaviour that constrains the scenarios

**TRAINING never serves the disc.** The default mode is a timed warm-up: the
disc stays parked in a wall slot, the opponent strolls in near the end, and
the round expires with nothing thrown. `disc_flight` and `tile_hit` therefore
run with `mode: challenge`, which is a real bout.

**A CHALLENGE round is short.** Standing still loses quickly -- roughly 300 to
500 frames from the cached savestate. Scenario windows have to fit inside
that. `collect.py` records `in_match` next to every dump so the analyzer can
tell when a dump was taken after the round ended, and prints
`(NOT in match!)` when it happens.

**The attract loop is not on a fixed schedule.** Waiting a fixed number of
VBLs for the menu is unreliable; SPACE is only accepted once the title screen
has finished loading. `navigate_to_match()` taps SPACE repeatedly and detects
each screen from the framebuffer instead (see the signature constants at the
top of `collect.py`).

## Accuracy of the timing

`wait_frames()` polls `evaluate VBL` over the control socket rather than
stopping the emulator, so a wait lands within about one frame of its target
rather than exactly on it. This is deliberate: stopping at a breakpoint would
drop Hatari into the debugger's readline loop, and the collector has no stdin
to get it back out. Every dump records the VBL it was actually taken at, and
`analyze.py` normalises deltas by the real gaps, so the imprecision does not
affect the analysis.

---

# Part 5 additions

## More Hatari 2.6.1 debugger quirks

**`memdump` ranges need a dash, not a space.** `m w $7616 $76b6` answers
`Invalid count 0!`; the syntax is `m w $7616-$76b6`. Same for `find` and
`disasm` takes the two-argument form.

**`b pc = $addr` breakpoints work -- my Part-5 note here was wrong.**
I recorded that a conditional breakpoint on `pc` was accepted but never
triggered. Re-tested in Part 6 against known-good addresses: `b pc > $1000`
fires immediately, `b pc = $8198` fires once per frame, and the `a <addr>`
shorthand does the same. The Part-5 failure was `b pc = $f650`, and `$f650`
is **not an instruction boundary** -- it came out of a chunked disassembly
that had misaligned (see the next entry). So the breakpoint was correct and
the address was wrong, which is itself a useful signal: if a `pc =`
breakpoint never fires on an address you believe executes, suspect your
disassembly alignment before you suspect Hatari.

**Chunked `disasm` misaligns.** Disassembling `$f400-$f600`, `$f600-$f800`, …
and concatenating produces plausible-looking but wrong instructions wherever a
chunk boundary lands mid-instruction — two different decodings of the same
addresses ended up in the same listing. Disassemble one continuous stream
instead: `disasm $a200` once, then bare `disasm` repeatedly, which continues
from where the previous one stopped. `scripts/collect.py` does not wrap this;
the probe scripts verify alignment by checking that known-good instruction
addresses appear in the output.

## The savebin sampling floor, and the way around it

Hatari services the control socket **once per emulated frame**, so every
`savebin` costs about two frames no matter how small the range. `--slowdown`
does not help — measured at slowdown 1, 4 and 8, the rate stayed at 0.5 dumps
per frame, because the cost is in frames, not wall time. This is the real
reason Part 4's `disc_flight` could only sample every ~14 frames and wrongly
concluded that no disc coordinate was stored.

The fix is to stop round-tripping: `lock memdump <addr>` plus
`b VBL ! VBL :trace :lock` makes Hatari write one memdump per VBL into its own
log, with no socket traffic at all. `Hatari.frame_trace()` wraps this and
returns one snapshot per frame; it captures whatever `nMemdumpLines` allows,
3200 bytes with the shipped `hatari.cfg`. Verified gap-free over 119
consecutive VBLs.

## Game behaviour

**A CHALLENGE round from the cached savestate lasts only ~300-500 frames.**
The player does nothing and loses. Traces have to fit inside that; `frame_trace`
windows of 60-120 frames are comfortable, longer ones are not. `in_match` is
recorded next to every dump so the analyzer can discard anything captured after
the round ended.

**`navigate_to_match` must not fire during floppy loads.** Blind fire presses on
a black loading screen get swallowed or land inside the match that is loading,
which is how an early CHALLENGE savestate ended up two seconds from the end of
a round. Navigation now checks for a dark screen and waits instead of firing.

## Not established

**No throw handler was identified**, because no scenario ever gets the player
into possession of the disc: TRAINING never serves it, and in CHALLENGE the
opponent starts with it and wins before our idle player touches it. What the
code shows instead is that all 8 disc records are created once per round by
the initialiser at `$aa50`, and thereafter a disc's X velocity is *steered*
(nudged +/-1 per frame toward a player-derived aim point, clamped to [-2,+2])
rather than re-launched. Whether a player-side "throw" writes the record
directly, or only sets the aim that the steering code converges on, is open.
Answering it needs a scenario that actually plays well enough to catch the
disc — the aiming problem that also blocked `tile_hit`.


---

# Part 6 additions (disc-oracle)

## Hatari

**The CPU profiler cannot be driven over the control socket.** `profile on`
followed by any wait reports "no activity" for every region. Profiling
collects only while the debugger is in control between an explicit continue
and the next breakpoint; each one-shot `hatari-debug` command enters and
leaves the debugger, which resets the buffers ("Freed previous CPU profile
buffers" appears on every command). Even a pure wall-clock sleep with no
debugger contact collected nothing. Use breakpoints and `--trace` instead --
`b pc > $dfffff :trace :lock` answered the "does it execute ROM" question in
one run.

**`memdump`, `find` and friends want a dash, not a space.** `m w $7616 $76b6`
answers `Invalid count 0!`; the syntax is `m w $7616-$76b6`.

**A breakpoint cannot be used to *stop* at an instant.** Arming
`b pc = $8198 :trace` and then sending `hatari-stop` lands wherever the
emulator is some milliseconds later -- measured as SR=$2309/IM=3 instead of
the $2404/IM=4 that holds at the VBL handler entry, i.e. already several
instructions into the handler. To capture state AT an instant, use the
breakpoint's `:file` action, which runs debugger commands at the hit. That is
how `Hatari.seed()` gets a frame-exact RAM image and register set.

## Musashi

**`m68k_end_timeslice()` does not abort the current instruction.** It zeroes
the cycle budget, which is tested at the top of the execute loop, so the
instruction that was about to run still runs. Stopping from the instruction
hook therefore lands one instruction late.

**`PC == $8198` is not observable from outside `m68k_execute()`.** The
interrupt dispatch and the handler's first instruction happen inside a single
call, so an external `if (PC == target)` loop stepping with `m68k_execute(1)`
always sees `$819c`. Both of these showed up identically -- as `$6ab4`
advancing by 2 per frame instead of 1 -- and both are fixed by *sampling* from
inside the instruction hook (which does fire pre-execution) rather than trying
to stop there.

## Emulating the IKBD

**The ACIA must not deassert its interrupt on acknowledge.** It stays asserted
for as long as a byte is waiting in the receive register, and the handler
clears it by reading `$FFFC02`. The game's decoder is a two-interrupt state
machine -- `$FF` re-points vector `$118` to `$83b2`, and the *next* interrupt
reads the joystick state byte -- so clearing on acknowledge means the second
interrupt never happens and `$6c58` never changes.

**Packets do not arrive on the frame boundary, and the offset is now
measured.** The IKBD is a 7812.5-baud serial device with no relationship to
the VBL. Two measurements:

* ACIA handler entry (`$8370`) is **uniform across the frame** -- 24 samples
  spread evenly from FrameCycles 15960 to 153184, deciles at roughly even
  spacing.
* The game **consumes `$6c58` at ~23200 cycles (scanline ~45)**. Found by
  bisecting `--ikbd-delay` against the Hatari reference: frames of exact
  agreement step from 61 to 364 between 23125 and 23312, and the high plateau
  extends to at least 159000.

Queuing at the frame boundary therefore makes the player react one frame early
-- `$6cae` = `$14` (walking) while the reference still shows `0` -- even though
`$6c58` itself matches, which is a confusing way for the bug to present. The
default delay is half a frame (80128), in the middle of the measured plateau.

**The residual is a genuine nondeterminism, not a modelling gap.** Because
arrivals are uniform and consumption is at 14.5% into the frame, about **one
real packet in seven lands before consumption and is acted on in the same
frame**. No fixed delay reproduces that per-packet coin flip; disc-oracle
trades it for reproducibility deliberately. It is one more reason input-heavy
programmes desync sooner than idle ones.

Neither of these is visible with an idle input script. Both appeared on the
first frame of the first scripted joystick run.

## Method

**Disassembling a handler's hot path is not disassembling the handler.**
Phase 0 read Timer A's 12-instruction loop, saw only PSG writes, and concluded
the timer could be skipped. Its exit path -- eight bytes further on, taken once
per sample -- clears two bytes below $8000. The differential phase found it
immediately. Read to the `rte`, on every branch.


---

# Part 8 additions

## The validated-window law (a permanent property, not a bug)

Agreement length between disc-oracle and Hatari falls as input density rises,
and always ends the same way: the video double-buffer pointers desync one
frame before everything else.

| programme | joystick changes | frames of tier-1 agreement |
|---|---|---|
| idle, from the original CHALLENGE seed | 0 | 275 |
| `leftright` | 4 | 256 (zero divergences; the run ended) |
| `rightpause` | 4 | 109 |
| `sweep` | 8 | ~364 |
| `rightfire` | 83 | 116 |
| idle, from the relayed `rally_f100` seed | 0 | 30 |

**It is input density that costs window, not scene activity.** The "quiet
seed" theory -- fewer live discs buys a longer window -- did not survive
measurement: live-disc counts inside every validated prefix show the quietest
stretch is always the *beginning*, because discs accumulate, so "quiet and
late" does not exist. Meanwhile `leftright` with four joystick changes held
256 frames while `rightfire` with 83 collapsed to 116. Keep programmes sparse.

## Realigning after a dropped frame does not work

Tempting and wrong. At a divergence the oracle really is one frame ahead --
shifting it back leaves exactly 4 differing bytes, all of them per-frame
counters (`$6ab4`, `$6ab6`, `$6c81`) off by exactly one. But the alignment
holds for a single frame and then collapses (64, 59, 116, 32 stray bytes on
the following frames). Once the game's own timing counters differ, the two
runs take different paths rather than the same path offset in time.
`oracle_diff.py --tier2` implements the realignment with a counter-confirming
guard and prints that evidence table when it refuses. It has never accepted a
realignment, and no result in this repo rests on one.

## Seeds must not be minted mid-press

A seed frozen while a key is held bakes `$6c58 = $80` into the image. A
reference trace then decodes the release a few frames later while a replay
running a different script never does, and verification fails on the joystick
byte for a reason that has nothing to do with emulation. `seed_relay.py` waits
for the decoded byte to settle, `seed()` records it as `joystick_6c58`, and
relay refuses to mint with input held.

## A nested key can shadow the one you are checking

A relayed seed records its parent's hash, so its JSON contains two `"sha256"`
keys. disc-oracle matched the first one it found -- the parent's -- and
rejected a perfectly good seed. The nested key is now `parent_sha256`, and the
check scans every occurrence rather than the first.


## Seed relay has a structural failure rate

Minting costs a 1 MB `savebin`, which takes about five emulated frames, and the
reference trace can only start afterwards. If Hatari drops a frame inside that
gap -- which it does, being the busier emulator -- the seed and the reference
start one frame apart and the oracle can never match from it. `quiet_f100`
failed exactly this way: oracle frame 4 matches the reference's frame 0 to
within the per-frame counters, frame 5 differs in 70 bytes.

There is no fix with the current capture path, so relay is best-effort: mint,
verify, and expect to discard some. `seeds/MANIFEST.md` records rejected seeds
rather than dropping them silently, because a rejected seed's **Hatari
reference is still ground truth** even though the seed is useless to the
oracle.

## Phase 0's "no FDC during a match" needs a qualifier

It was measured over 30 frames early in a round and is true there. Run long
enough for the round to *end* and the game does touch the FDC: disc-oracle
aborted on an unstubbed `write.w $ff8606` (DMA mode/status) at PC `$000162`
during a 400-frame `leftright` run. The abort-by-default rule caught it instead
of returning 0 and producing a plausible-but-wrong trace. Keep oracle runs
inside the round, or stub the FDC before extending them past it.

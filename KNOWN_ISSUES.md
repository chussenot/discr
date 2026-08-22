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

**`b pc = $addr` never fires.** A conditional breakpoint on the `pc` variable
was set and acknowledged (`CPU condition breakpoint 1 with 1 condition(s)
added`) but never triggered on an address that provably executes every frame.
Not chased down; the working alternative is a change-tracking breakpoint on a
memory location the routine writes (`b ($addr).w ! ($addr).w :trace :lock`),
which reports the machine state at the instruction *after* the write, plus
`lock registers` when the address registers are what you are after. That is
how `a0` was read out at the movement guards.

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

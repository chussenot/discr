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

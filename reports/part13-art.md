# Part 13 (art) — original samples wired in; original graphics not proven this pass

bd `discr-rxx.7`. Scope was "put the original game's assets into `disc-app`
with what is provably available today" (wave 7: `DECOR00.DAT`, `BONUS01.DAT`,
`VIC.DAT` proven raw; all 11 `.SPL` samples; `CONVERTX/Y.DAT` LUTs).
Delivered: the nine mapped `.SPL` samples, decoded at runtime and wired to
real gameplay events/state transitions. Not delivered, with the negative
evidence recorded rather than a guess shipped: a proven pixel/vector format
for `DECOR00`/`BONUS01`/`VIC.DAT`, and a live-verified consumption site for
`CONVERTX/Y.DAT` — both explained below, both filed as follow-up work rather
than forced into the renderer per house rules ("a hypothesis is proven by a
rendered image that looks right", "never guess the algorithm into the code").

## 1. Audio: all nine mapped `.SPL` samples, decoded at runtime

`crates/disc-app/src/audio.rs` (new). No build script, no shelling out to
`dscfs`, nothing derived committed: `assets/original/*.SPL` bytes are read
with `std::fs::read` at startup and wrapped in an in-memory WAV container
(`spl_to_wav16`) built byte-for-byte the same way `crates/disc-tools/src/
bin/dscfs.rs`'s own `pcm8_to_wav16` does — `sample * 256`, exact, no
clipping — duplicated locally (not imported from `disc-tools`) so `disc-app`
does not pull in `disc-tools`' dependency tree, matching this crate's
existing "the only dependency" policy in its `Cargo.toml`. The resulting
bytes go straight to `macroquad::audio::load_sound_from_bytes` (macroquad's
`audio` feature, newly enabled in `crates/disc-app/Cargo.toml`, pulls in
`quad-snd`/`audrey`/`hound`/`lewton` — all fetched cleanly through the
configured registry proxy).

Event wiring (`audio::Cue`, matched in `MatchState::cue_core_events` /
`cue_state_edges` in `main.rs`):

| `.SPL` | `Cue` | Trigger |
|---|---|---|
| `LAUNCH.SPL` | `Serve` | `disc_core::Event::DiscServed`, and `MatchState::serve_workaround` on a successful workaround-served disc (the workaround calls `disc::serve` directly, bypassing the event list — see that method's own doc) |
| `PARADE.SPL` | `Block` | `disc_core::Event::DiscReflected` |
| `DESDALLE.SPL` | `TileDestroyed` | `disc_core::Event::TileDestroyed` |
| `IMPACT.SPL` | `Impact` | `disc_core::Event::TileDamaged` |
| `MORT.SPL` | `Death` | a player's `state_index` entering `STATE_DEAD`, edge-detected tick-to-tick |
| `VICTOIRE.SPL` | `Win` | `round::Phase` transitioning into `GameOver` |
| `GONG.SPL` | `Round` | a fresh round dealt (`RoundOver` -> `Playing`) |
| `TOUCHDEF.SPL` | `DefendedHit` | a player's `state_index` entering `STATE_INTERCEPT` or `STATE_CATCH19` |
| `CHUTE.SPL` | `Fall` | a player's `state_index` entering `STATE_STRUCK_DOWN` or `STATE_STRUCK_UP` |

The first four are real `disc_core::Event`s returned by `GameState::tick`.
The other five have no core event to hang off — `disc-core`/`round.rs`'s own
module docs say plainly that death, round-over and win/loss bookkeeping live
in the app, not core — so they're edge-detected against `state_index`
transitions this crate already has on hand (`self.prev` vs `self.cur`,
exactly the snapshot `step_tick` already keeps for render interpolation).
None of these five mappings is a decode of which ST code site queues which
sample (that would need the same live PC-breakpoint tracing the depack
report used for `LAUNCHER.HA`, not attempted here); each is `audio::Cue`'s
own doc-commented, app-level policy call, same status as `round.rs`'s other
undecoded choices (round-winner policy, round-win count, etc.). `DIC13.SPL`
and `VITRE15K.SPL` are `dscfs`'s own "unknown (undocumented)" pair — loaded
by nothing, cued by nothing.

**Missing assets**: `load_one` catches both `std::fs::read` and
`load_sound_from_bytes` failure per file, logs a warning, and leaves that
`Sfx` field `None` — `Sfx::play` is then a silent no-op, never a panic. A
clone without `assets/original/` (or with one `.SPL` missing) still runs.
`disc-app` stays out of the workspace `default-members` for its own,
separate reason (system deps — libGL/libX11/libasound), unchanged by this
work; no asset is read at compile time (no `include_bytes!`), so a missing
`assets/` cannot break the build either way.

**Sample rate**: `RATE_HZ = 8000`, the same documented-as-a-guess default
`dscfs samples` uses — the real Timer A replay rate (`docs/disc-notes.md`:
"~4.9 kHz PSG sample streamer", for the *live-match* streamer, not
necessarily these offline assets' own authored rate) is not reconstructed.

**Tests** (`cargo test -p disc-app`, 37 passed, 0 failed): `audio::tests::
wav_header_is_well_formed_and_scale_up_is_exact` checks the WAV container's
RIFF/fmt/data chunk layout and the exact `*256` scale-up on five boundary
byte values (`0x00, 0x01, 0xFF, 0x80, 0x7F`); `audio::tests::
playing_an_unloaded_cue_is_a_silent_no_op` exercises every `Cue` variant
against a `Sfx::default()` (all `None`) and asserts no panic.

**Runtime verification**: run under Xvfb (`tmp/shots/app.log`, this
worktree), all nine `.SPL` files were found and **decoded successfully**
(`load_sound_from_bytes` returned `Ok` nine times — zero `"disc-app: audio:
... unavailable/failed to decode"` warnings in the log) — the WAV bytes this
crate builds are valid input to macroquad's own decoder end to end. The
*playback* device itself is unavailable in this sandboxed container (no
ALSA card: `quad_snd`'s own audio thread panics on `Can't open PCM device`
and dies, logged 16 times), which is an environment limit, not a code
defect — the panic is confined to that background thread and does not take
the process down: the app kept rendering and progressing through rounds
after it (see screenshots below). On a real desktop with a sound card this
plays normally; nothing in this crate's own code path depends on a device
being present at load time.

## 2. Screenshots (this worktree's `tmp/shots/`, not committed)

Driven the way house rules ask (Xvfb + a screenshot tool — `import`/
ImageMagick here, against the app's own window, since `disc-app` is a
macroquad/miniquad window, not a Hatari instance dscfs's `screenshot`
debugger command could target):

- `tmp/shots/01_menu.png` — title screen, `TRAINING`/`CHALLENGE` menu.
- `tmp/shots/02_rally_start.png`, `03_rally.png`, `04_rally_later.png` — a
  live training match: the placeholder two-platform arena (unchanged this
  pass — see §3 for why), a player rectangle, a disc, HUD, and (caught live
  in `03_rally.png`) a `"ROUND TO P2"` banner — the app reached at least two
  full `Playing` -> death -> `RoundOver` -> fresh-round cycles in the few
  seconds of the capture window, exercising every state-edge cue
  (`Death`/`Round`, and by construction `TileDamaged`/`TileDestroyed`/
  `DiscReflected`/`DiscServed` during the rallies themselves) without a
  crash. `GameOver`/`Win` was not separately screenshotted (reaching it
  needs `GAME_OVER_ROUND_WINS` = 3 round wins, more capture time than this
  pass spent) but is exercised, headless, by the existing
  `an_unattended_challenge_match_reaches_game_over` test, which now runs
  with `self.sfx` wired at every transition the new code touches.

Placeholder rendering (flat-colour rectangles/bars from `main.rs`'s existing
`draw_bank`/`draw_match`) is otherwise **unchanged** — see §3.

## 3. `DECOR00.DAT` / `BONUS01.DAT` / `VIC.DAT`: not proven, evidence recorded

The task's own house rule: a pixel-format hypothesis is proven by a
rendered image that looks right against the real game, not asserted. None
of the three cleared that bar this pass, so none is wired into the
renderer — the arena backdrop, bonus icons and victory graphic all stay on
`main.rs`'s existing placeholder shapes. What was actually tried, and what
it ruled out:

**Live ground truth captured first.** `scripts/collect.py`'s `Hatari`
class, loaded from the cached `tmp/match_training.sav` savestate (no need
for a cold ~40s boot), gave a real training-match screenshot and the live
16-colour palette read directly off hardware registers (`Hatari.dump`
targeting `$FF8240..$FF8260`, 16 words, decoded as the standard ST 9-bit
`0RGB` format). The screenshot shows the real backdrop: jagged low-poly
purple mountain silhouettes either side of a starfield sky, an orange/purple
checkerboard floor (both banks, 4x2 — matching `main.rs`'s own measured
`BANK_COLS`/`BANK_ROWS`), and — importantly — eight wall-mounted icon boxes
above the far bank (one lit orange disc icon, seven "∞" symbols), which read
as a per-cell status readout, not decoration; a strong candidate for what
`BONUS01.DAT` actually is (icon frames for that readout), separate from
whatever `DECOR00.DAT` turns out to be (the mountains/sky).

**Bitplane raster, tried both interleaved and planar-sequential, many
width/header hypotheses, all noise.** ST low-res is 4 interleaved bitplanes,
16px words (house rules' own stated ground truth). `DECOR00.DAT` (7477 B),
`BONUS01.DAT` (2749 B) and `VIC.DAT` (17908 B) have **no** exact `(header=0,
width, height)` fit at 4bpp at all (checked every 16px-multiple width
16..336) — already a bad sign for "plain ST raster, no header". Allowing a
header up to 64 bytes turns up plenty of exact-byte-count candidates for
each file (a dozen-plus apiece); every sensible one tried for `VIC.DAT`
(the best raster candidate — see the entropy argument below) — both as
word-interleaved planes and as four sequential whole-bitmap planes, using
the real captured palette — decoded to visual noise, not a picture. A
synthetic self-check (a hand-built 32x8 diagonal-band test image, same
interleaved encoding) round-tripped through the same decoder correctly,
which rules out a bug in the decoder itself as the explanation.

**Byte statistics separate `VIC.DAT` from the other two, and from
`CONVERTX/Y.DAT`.** `VIC.DAT` uses all 256 byte values (mean abs signed
value ~41) — real-picture-like entropy, no significant autocorrelation at
any lag 2..60 (peak 0.12) — consistent with being genuine raster pixel
data whose exact geometry/header this pass simply didn't hit.
`DECOR00.DAT` (193/256 unique values) and especially `BONUS01.DAT` (17/256
unique values) do not look like raster data at all: both are dominated by
small signed deltas clustered near zero, and both show **strong
periodicity** — `BONUS01.DAT` peaks at lag 12 (autocorrelation 0.96,
matching `CONVERTX.DAT`/`CONVERTY.DAT`'s own lag-12 peaks of 0.98/0.97,
see §4) and `DECOR00.DAT` peaks around lag 18/37/55 (0.90-0.93, still
strongly periodic, just a different period). This is evidence, not proof,
but it points the same direction as the jagged, angular mountain art
actually on screen: `DECOR00.DAT` and `BONUS01.DAT` read statistically
like the *same family* of compact, periodic coordinate/offset tables as
`CONVERTX/Y.DAT` — plausibly a procedural line-drawing input (a repeating
angle/offset unit tiled to build the skyline, or the eight wall-icon
positions), not a bitplane bitmap at all. `VIC.DAT` reads like the odd one
out — genuinely raster, format still unresolved.

**Not chased further this pass**: which code reads `$58B12`/`$442D2`/
`$1F900` at draw time (this would settle it directly, the way the depack
report settled `LAUNCHER.HA`'s NSQ loop, by watching the read/PC live —
not attempted here for time). Filed as follow-up (see §5).

## 4. `CONVERTX.DAT` / `CONVERTY.DAT`: confirmed non-linear, not integrated

`disc-app`'s current renderer (`main.rs`, unchanged by this pass) uses **no**
projection at all for world-to-screen mapping — a straight linear scale
(`View::s`) plus a per-player assumed vertical band (`y_in_band`); its own
doc says plainly it deliberately does not reimplement the ST's
`$a6b2`/`$a6b6` LUT-based perspective projection. So the comparison the task
asked for ("compare the LUT curves against disc-app's isometric
projection") has an unambiguous answer: **they differ** — the app's mapping
is linear/monotonic by construction, `CONVERTX.DAT` (2094 B) and
`CONVERTY.DAT` (1071 B) are not. Both are strongly periodic (autocorrelation
peak 0.980 at lag 12 for X, 0.974 at lag 12 for Y — computed by centred
autocorrelation over lags 4..60, both dominant peaks are at exactly the same
lag), amplitude roughly ±100, i.e. a repeating ~12-sample wave shape tiled
across the table — the signature of a discretised sine/cosine-style angular
LUT, not a monotonic per-depth ramp. `X ≈ 2×Y`'s byte-count ratio
(`docs/loriciel-formats.md` §4) matches this: 2094/12 ≈ 174.5 repeats for X
against 1071/12 ≈ 89.25 for Y, roughly double, consistent with the doubled
horizontal LUT resolution the docs already attribute to ST screen aspect.

**Not switched into the renderer.** The task's framing ("switch the app's
projection to the LUTs — they are the original's exact math") presupposes
knowing which index selects which output and what the output represents.
`reports/findings.md` (an earlier phase) names `disc_project`'s (`$a6b2`)
four LUTs as `$7abe`, `$7b5e`, `$59952`, `$5b252` — but resolves the first
two as **simple linear ramps** (`$7abe[i] = i*80`, `$7b5e[i] = i*40`), not
`CONVERTX/Y.DAT` at all, and the other two addresses land, in the *later*
depack report's own RAM map, **inside** `DECOR00.DAT`'s and `DECOR01.DAT`'s
own resident spans (`$59952` is `DECOR00`+3648, `$5b252` is `DECOR01`+2304)
— almost certainly a snapshot/build-order mismatch (the same kind of
staging-buffer reuse Finding 1 of the depack report already caught for
`PROGRAM.HA`/`DECOR01`'s own tail), not evidence `CONVERTX/Y.DAT` actually
lives there. Concretely: I do not have a live-verified answer for *where*
`CONVERTX/Y.DAT` get consumed, so integrating them into `main.rs`'s
rendering would be guessing the algorithm into the code, which house rules
rule out. Recorded here as a confirmed-different, not-yet-integrated
finding.

## 5. Follow-up filed

Both graphics-format gaps above need the same tool the depack report already
proved works for this codebase: a live Hatari session with a PC/read
breakpoint on the actual draw routine (watch reads sourced from `$58B12`/
`$442D2`/`$1F900`, or a PC breakpoint on whatever calls
`CONVERTX.DAT`/`CONVERTY.DAT`'s resident copies), not further static
guessing. Left as open follow-up under this same bead (kept `in_progress`,
not closed — see §6) rather than a new bead, since it is squarely this
bead's own remaining scope.

## 6. Bead status

`discr-rxx.7` stays `in_progress`. Everything provably available and
actually wired: the nine mapped `.SPL` samples, decoded at runtime, cued off
real `disc-core` events and this crate's own already-tracked state
transitions, with a graceful silent fallback when an asset is missing.
Everything provably available but **not** wired, with the evidence for why
recorded above rather than guessed into the renderer: `DECOR00.DAT`,
`BONUS01.DAT`, `VIC.DAT` (pixel/vector format not cracked this pass) and
`CONVERTX.DAT`/`CONVERTY.DAT` (confirmed non-linear and different from the
app's current math, consumption site not live-verified). The still-packed
remainder (`DALLES01`/`PLAYER01`/`ENEMY01`/`DECOR02-04`, owned by `depack`)
is unchanged and was not the blocker for anything in this report.

## Gates run

```
cargo fmt --check                                                        -- clean
cargo clippy -p disc-app --all-targets -- -D warnings                    -- clean
cargo test -p disc-app                                                   -- 37 passed, 0 failed
cargo run -q -p disc-tools --bin tracecheck -- tests/fixtures/golden.ndjson --min-agree 99
                                                                           -- OK: 99 tick(s) matched (untouched)
```

## Files

- `crates/disc-app/src/audio.rs` — new. `.SPL` -> in-memory WAV -> macroquad
  `Sound`, the `Cue` enum, `Sfx`, tests.
- `crates/disc-app/src/main.rs` — `MatchState` gains an `sfx: Sfx` field and
  `with_sfx` builder; `step_tick` captures `GameState::tick`'s events and
  cues them (`cue_core_events`), edge-detects the five app-level cues
  (`cue_state_edges`), and cues `Win`/`Round` on the two phase transitions;
  `serve_workaround` cues `Serve` on a successful workaround serve; `main`
  loads `Sfx` once before the menu loop and clones it into each new
  `MatchState`. All existing rendering, tests and the `MatchState::new`
  signature are untouched.
- `crates/disc-app/Cargo.toml` — `macroquad`'s `audio` feature enabled
  (pulls `quad-snd`/`audrey`/`hound`/`lewton`/`quad-alsa-sys`; no other new
  direct dependency).
- `reports/part13-art.md` — this file.

Not committed: `assets/` (read-only, owned by `orchestrator`, unchanged);
`tmp/shots/*.png` and `tmp/shots/app.log` (this worktree's own `tmp/`,
gitignored, referenced above by path).

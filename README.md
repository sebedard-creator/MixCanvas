# MixCanvas

**A DJ mix editor for people who would rather build a set than perform one.**

MixCanvas is free, open source software for Windows. You drop tracks onto a
musical timeline, line them up, draw what they do, and hear the result straight
away. Nothing is rendered in advance: the audio engine, written in Rust, works
it all out during playback.

Decks reward the right gesture at the right moment. MixCanvas offers the
opposite — you can go back to a transition, take it again, compare, until it is
the one you meant.

**Nothing leaves your machine.** The program makes no network request of any
kind. Beat detection and vocal separation run locally.

---

## The timeline

Three stereo tracks, a ruler in bars and beats, continuous zoom on the wheel.
Tracks come in from the library by dragging, or with a click that takes the
first lane actually free at the playhead.

A clip moves horizontally to change when it plays and vertically to change
lane — **including during playback**, without a gap. Its first downbeat snaps to
four-beat bars on its own, and the pre-roll, whatever comes before the track's
first beat, stays visible rather than clipped.

The cursor tells you what a click will do from where you hover: move in the
middle, trim near an edge. `B` splits a clip into two independent halves,
`Delete` removes the one under the pointer, and right-clicking its name opens
its menu.

Removing a clip **takes with it the automation that only existed for it** — but
not a neighbour's, where one shares the same span.

## Tempo

Every clip puts a **tempo target** on the global curve — the turquoise node —
which you can drag along the ruler. Between two targets the tempo moves **in
whole-beat steps**: it holds for the length of a beat and only changes at the
boundary, where the transient covers it. A tempo change mid-beat is audible; on
the beat, it is not.

Right-clicking a node sets **the speed this clip plays at here**. That is not
the track's BPM: the track keeps whatever the analysis says, and the clip is
stretched towards the target instead. *Follow track* gives it its native speed
back.

Stretching preserves pitch — no varispeed. A stereo-linked WSOLA engine looks
for a correlated waveform before each splice and applies a cosine crossfade,
within a range of 0.5× to 2×.

## Finding the beat

On import, every track goes through **Beat This!**, a neural beat tracker, run
locally. Its events are then fitted to a rigid grid — a DJ needs a constant
clock, not a list of musical events — and the first downbeat is placed where the
kick actually enters, so an ambient intro does not push the grid off.

When the model cannot run, a correlation-based analyser takes over. Either way
the result is only a starting point: the **Beatgrid Editor** lets you tap the
tempo, nudge it, halve or double it, set the first downbeat from what you hear —
and hand everything back to the automatic analysis.

A manual correction **survives** re-analysis. It represents your work, and the
program never overwrites it on its own.

## Automation

Three lanes per track, drawn straight onto the timeline.

**Volume**, from −∞ to +12 dB. **Pan**, at constant power. **Filter**, bipolar:
upward is a high-pass, downward a low-pass, the middle is bypass.

You can place nodes one at a time, or draw a whole shape: `V` and `P` pick the
lane, `S` the shape — step, sine, triangle — and `D` its period. The filter band
takes the bubble brush, a symmetrical triangle with `Shift`, and freehand with
`Ctrl`.

Every curve can be resized by its edges and deleted with a right click.

## Effects

A **three-band EQ per clip**, adjustable while it plays.

On the master bus, in console order: **sidechain compression** — one clip
becomes the key, goes quiet where it overlaps others and pumps them instead — a
**glue compressor**, **console colour** with its saturation, metering, then a
stereo-linked **limiter**. The `OL` lamp is measured *after* the limiter: it
only lights on clipping you actually suffered.

The analogue VU meter in the middle reads the real float32 output.

## Vocals and instrumental

**Open-Unmix** splits a clip into vocals and instrumental, locally. Each clip
then chooses what it plays: the whole track, vocals alone, instrumental alone.
Only the window the clip actually uses is separated, so you do not wait on a
passage you will never hear.

## Baking a clip

`BAKE` renders a clip **on its own, with its effects**, into a file. The clip
then plays that file instead of recomputing its chain on every pass, which
lightens a busy mix.

It is **reversible**: the automation removed at bake time is kept exactly as it
was and handed back when you thaw. A button with no way back stops being
pressed.

## Playing, saving, exporting

`Space` starts and stops. A click in the timeline places the playhead, and
`AUTOPLAY` decides whether that click also starts playback — handy when you are
placing clips by ear.

Nothing is decoded in advance: each clip opens its file when it becomes active
and keeps only a window around the current position. Changing a tempo, moving a
clip or jumping through the mix therefore needs no render.

`SAVE` and `LOAD` write a portable `.mixcanvas` project. **`BOUNCE MIX`**
exports the whole mix as 16-bit / 44.1 kHz stereo WAV, with TPDF dither.

Undo history holds fifty levels — `Ctrl+Z`, `Ctrl+Y`.

---

## Where your files live

**Everything MixCanvas writes lives in one folder beside the program**, called
`MixCanvas Files`: the library database, the resources unpacked from the
executable, and the WAV files for separated stems and baked clips.

Copy the program and that folder and you have moved your whole setup. Delete the
folder and you are back to a fresh install. Nothing is hidden away in
`%APPDATA%`.

Each project gets a folder of its own; an unsaved session lives in `Scratch`
until you name it, and its media follow. On exit, files that nothing refers to
any more are deleted — a file still referenced is never touched.

Your MP3s stay where they are. The library keeps only their path and metadata,
and can tell you which ones have gone missing or moved.

*One exception: a program placed somewhere it may not write — `Program Files`, a
read-only share — falls back to the application data folder, because refusing to
start would be worse.*

## How the interface draws

MixCanvas draws its interface **in software**, deliberately. On the machines we
profiled, hardware acceleration made no measurable difference, while some
graphics drivers make WebView2's compositor tear during a zoom. Given a choice
between no gain and a possible artefact, correctness wins.

Nothing about your audio depends on this. Playback, analysis, mixing and
bouncing are native Rust and never touch the browser's renderer.

The mode is chosen at launch, with no rebuild — the last flag wins:

| Flag | Effect |
|---|---|
| *(none)* or `--no-gpu` | everything in software — the default |
| `--gpu-safe` | the card draws, compositing stays in software |
| `--gpu` | full hardware acceleration |

```powershell
.\MixCanvas.exe --gpu-safe
```

The `portable` folder ships a `.cmd` shortcut for each mode, which picks up the
most recent build sitting beside it.

---

## Building from source

You need Node.js with Corepack, the Rust toolchain named in
`rust-toolchain.toml`, the Microsoft C++ Build Tools with the "Desktop
development with C++" workload, and WebView2 — already present on recent
Windows.

```powershell
.\install.cmd    # dependencies, kept inside the repo: nothing installs globally
.\dev.cmd        # run in development
.\check.cmd      # tsc, tests, cargo test, fmt, clippy
```

`install.cmd` fetches pnpm into `.corepack` and keeps the caches inside the
repository. Nothing touches your `PATH`. Both beat-tracking models are part of
the repository, so a fresh clone can analyse without any download.

**Close MixCanvas before `check.cmd`** — the running executable locks the ONNX
Runtime DLL it embeds.

The single-file portable build is made like this:

```powershell
npx tauri build --no-bundle -f embed-resources
```

`embed-resources` bundles the models and ONNX Runtime into the executable.
Without it the binary carries neither the frontend nor the models.

Verified for 1.0.0: TypeScript, 230 frontend tests across 27 files, 174 Rust
tests, formatting and Clippy — `check.cmd` exits 0.

## Under the hood

- **Tauri 2**, React and TypeScript interface, Rust engine.
- **Symphonia** through **Rodio** for decoding, **CPAL** for native output.
- Embedded **SQLite**, no server: library, timeline and automation.
- **RTen** runs Beat This!, **ONNX Runtime** runs Open-Unmix.
- The mix is computed in `f32` throughout, at the device's real sample rate, to
  avoid a double 44.1 ↔ 48 kHz conversion.

`architecture.md` explains the *why* behind each technical decision. It is
written in French, as are the source comments.

## Licence

MixCanvas is released under the [GNU AGPL version 3 only](LICENSE), identified
by the SPDX expression `AGPL-3.0-only`.

You may use, study, share and modify it. If you distribute a modified version —
including over a network — you must offer its source code under the same
licence.

Forks are welcome. The licence already asks you to keep the copyright notices
and to state what you changed. Beyond that, and as a courtesy rather than a
condition, naming MixCanvas and linking back to the original project would be
genuinely appreciated.

`THIRD_PARTY_NOTICES.md` holds the licences and SHA-256 digests of the models
and binaries shipped with the program.

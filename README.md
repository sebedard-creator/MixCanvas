# MixCanvas

**Build DJ mixes with the precision of a timeline and the feel of a studio.**

MixCanvas is a free, open-source Windows application for creating complete DJ
mixes before the party starts. Import your own MP3s, correct the beatgrid when
needed, arrange tracks on a musical timeline, shape transitions with automation,
and play or export the finished mix.

It is deliberately focused: no streaming service, no account, no cloud library,
and no VST plugin ecosystem. Your music, your files, your computer.

## Why MixCanvas

Traditional DJ software is made for performing in the moment. MixCanvas is made
for refining a mix over time. Move a transition, hear it again, draw the change
you want, and keep editing while the timeline plays.

- Three stereo tracks on one beat-based timeline.
- Automatic BPM and downbeat analysis, with practical manual correction.
- Pitch-preserving time-stretching and a global tempo curve for beatmatched
  transitions.
- Four effects you play by hand while the mix runs, recorded onto the timeline.
- Local audio processing in 32-bit floating point.
- No subscriptions, no telemetry, and no network connection required.

![MixCanvas: the console with transport, VU meters and Mix FX pads above three stereo tracks carrying clips, waveforms, tempo targets and automation, with the Library on the right](assets/screenshots/timeline.png)

*The whole mix at a glance: console across the top, three lanes of clips,
waveforms and automation below, Library on the right. A fifty-eight minute set,
all of it editable while it plays.*

## Core workflow

1. Add MP3s to the Library.
2. MixCanvas reads ID3 artist/title data, creates a waveform, and analyses BPM,
   beatgrid, and first downbeat in the background.
3. Preview a track, correct it in the Beatgrid Editor if required, then drag it
   onto the Timeline.
4. Arrange, beatmatch, automate, play, save, and bounce the finished mix.

## Library and beatgrid

- Import individual MP3 files, whole folders, or drag MP3s in from Explorer.
- Library display uses Artist – Track Title when metadata is available.
- Sort by BPM, artist, title, or timeline use; used tracks receive a visual
  overlay to prevent accidental duplicate additions.
- Per-track preview player with seekable waveform/progress control.
- Local automatic BPM, beat, downbeat, and waveform analysis.
- Beatgrid Editor with preview playback, manual BPM, half/double tempo tools,
  Tap 1 downbeat capture, Snap to Beat, and restoration of the automatic result.
- Manual corrections remain yours: re-analysis does not overwrite them.

![Beatgrid Editor: manual BPM, Tap 1, Snap to Beat, first downbeat, and re-analysis controls](assets/screenshots/beatgrid-editor.png)

*When automatic analysis needs help, the Beatgrid Editor makes the correction
audible, guided, and repeatable.*

## Musical timeline

- Three stereo lanes: A, B, and C.
- Drag-and-drop placement or one-click insertion into the next free lane.
- Beat/bar snapping keeps downbeats aligned while clips move horizontally or
  vertically — even during playback.
- Continuous zoom, full-project zoom-out, centered playhead follow, click-to-seek,
  and a horizontal navigator.
- Drag clips, trim either edge, split a clip, remove it, or undo/redo up to
  fifty edits.
- Floating Mute and Solo controls per lane.
- Global tempo map with draggable tempo targets. Between two targets the tempo
  moves in whole-beat steps rather than sliding continuously, so a tempo change
  lands on a beat where the transient hides it instead of drifting audibly.
- Right-click a tempo target to set the speed that clip plays at, without
  touching the track's analysed BPM.
- Right-click a track in the Library, or a clip on the timeline, to shift its
  downbeat a beat either way — for when analysis puts the bar line on the 2 or
  the 3. The grid is right, it is only turned; the tempo does not move.
- Every clip follows the project tempo through pitch-preserving stereo-linked
  time-stretching, rather than varispeed, across a safe 0.5×–2× range.
- Play/Pause transport, Spacebar control, optional Autoplay, and live timeline
  editing without pre-rendering the entire set.

## Automation and transitions

MixCanvas lets you draw transitions directly where they happen.

- Volume automation from −∞ dB to +12 dB.
- Pan automation with a constant-power pan law.
- Smart Filter automation: draw high-pass or low-pass sweeps as visual curves,
  reshape them, and remove them with a right click.
- **Nodes** are for exact, individual control points: place, move, or delete a
  specific value at a specific beat.
- **Draw** is for a complete musical gesture. Drag across a range to create a
  step, sine, or triangle Volume/Pan movement with its own period and shape.
  It appears as one clean curve, can be deleted in one action, and does not
  clutter the timeline with hundreds of visible nodes.
- Under the hood, Draw still preserves the detailed automation points required
  by the audio engine. The simplified line is only a clearer way to edit it.
- Automation belongs to the lane, not to the clip: it stays at the beats where
  you drew it when a clip moves. Removing a clip does clear the automation that
  was only its own, without touching a neighbour's.

## Play effects by hand

Five pads per track sit in the console, beside the transport. Hold one while
the mix runs and the effect goes onto that track; let go and the pass is
written onto the timeline as automation you can keep, undo, or replay over.
Nothing to open and nothing to put away.

<p align="center">
  <img src="assets/screenshots/mix-fx.png" width="344"
       alt="The Mix FX bay: rows for tracks A, B, and C, each with reverb, flange, crush, delay, and eraser pads. Delay is lit orange on A and reverb lit purple on B.">
  <br>
  <em>Five pads per track, in the console beside the transport. A pad lights
  while its effect is sounding — here delay on A and reverb on B.</em>
</p>

- **Reverb** — a shared room. Bright and generous, sitting well behind the
  music.
- **Flange** — a comb filter sweeping across the track, ping-ponging in stereo.
- **Crush** — eight-bit quantisation with sample-and-hold aliasing, replacing
  the track while you hold it.
- **Delay** — a dotted-eighth echo that follows the project tempo, so it stays
  on the beat even through a tempo ramp. It bounces from one ear to the other
  and each repeat comes back darker. Hold it, cut the track, and the echo
  carries the transition.
- Reverb, flange, and delay sit on sends, so their tails keep ringing after you
  release the pad. Crush sits inline, and the other three are taken after it —
  you hear the room around the crushed sound.
- The pass draws itself while you play it: a coloured band grows from where you
  pressed and follows the playhead. Purple for reverb, green for flange,
  magenta for crush, orange for delay. Where two overlap, the region is hatched
  in both colours.
- An eraser pad on each track wipes every effect it sweeps over, in one undoable
  step. The track falls silent under it as you go, so you hear what you are
  removing while you remove it.
- A pad lights whenever its effect is sounding — under your finger, or under the
  playhead when a recorded pass plays back.
- The pads have no keyboard shortcuts. With a mouse that means one effect at a
  time, which is the honest trade: the fifteen shortcuts they used to carry
  collided with the timeline's own.

## Sound and mix tools

- High-definition stereo waveform display with peak and RMS detail, adapted to
  the current zoom level.
- Three-band EQ on every clip, adjustable during playback.
- Sidechain compression: make a clip the key and let it pump overlapping clips.
- Master Glue Compressor with console colour/saturation.
- Stereo-linked master limiter and a real post-limiter OL overload lamp. This
  one is a safeguard for listening, and it stays out of the way of the
  mastering limiter on the bounce.
- Vintage stereo VU meters driven by the actual output signal.
- All decoding, time-stretching, automation, mixing, and processing run in
  float32 at the audio device's native sample rate.

| Per-clip EQ | Reversible Bake |
|---|---|
| ![Three-band Clip EQ with high-pass, parametric bell, low-pass, and gain controls](assets/screenshots/clip-eq.png) | ![Bake progress dialog while rendering a clip and its effects](assets/screenshots/bake.png) |
| Shape a clip while it plays. | Render a dense clip when you are ready, then undo it at any time. |

## Vocals, instrumentals, and baking

- Separate a clip into Vocals and Instrumental locally with Open-Unmix.
- Choose full track, vocals only, or instrumental only per clip.
- Separation only processes the part of the source used by the clip, not an
  entire song unnecessarily.
- BAKE renders a clip and its effects into a reusable audio file to lighten a
  dense mix. Baking is reversible and restores the automation it replaced.

## Save and export

<p align="center">
  <img src="assets/screenshots/bounce.png" width="512"
       alt="The Bounce the mix dialog: WAV and MP3 format cards, an armed Mastering Limiter with threshold, ceiling, and release fields, auto release, and a line reading Lifts the mix by 2.7 dB, then holds it under -1.0 dB.">
  <br>
  <em>One dialog before the render: pick the format, and the limiter tells you
  what it is about to do before it does it.</em>
</p>

- Save and load portable .mixcanvas projects.
- Bounce the complete mix to 16-bit / 44.1 kHz stereo WAV with dither, or to
  44.1 kHz CBR 320 kbps stereo MP3 encoded with LAME at its most thorough
  setting. The mix reaches the encoder in floating point, so an MP3 is never
  built from an already-quantised file.
- **A mastering limiter on the bounce, armed by default.** A brickwall with
  look-ahead: it sees the peak coming and has already lowered the gain when it
  arrives, so nothing gets past the ceiling and transients are limited rather
  than clipped. Uncheck it if you master the file elsewhere. Lowering the threshold lifts the whole mix to the ceiling, and
  the dialog says by how many decibels. Its release can follow the programme —
  quick after a lone peak, slow through a dense passage. Arming it renders
  without the playback safeguard, which would otherwise clip the transients
  first. Choosing MP3 lowers the ceiling a decibel, because a lossy codec
  rebuilds peaks slightly higher than the file it came from.
- The original MP3 files are never modified.
- The Library only stores references and metadata, and warns when a source file
  was moved or is missing.

## Private and portable by design

MixCanvas makes no network request. BPM detection, waveform generation, vocal
separation, and all audio processing run on your machine.

Everything MixCanvas creates lives in MixCanvas Files beside the executable:
the database, extracted resources, separated stems, baked clips, and project
media. Copy the executable and that folder to move your setup. Your original
MP3s always stay where you put them.

### Windows requirement

MixCanvas uses the **Microsoft Edge WebView2 Runtime** to display its interface.
It is already included with Windows 11 and most supported Windows 10 systems.
If it has been removed or is unavailable, install the
[WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).
You do not need to install or use the Microsoft Edge browser itself.

The portable build draws with the GPU by default. Software drawing held that
place until 1.6, because WebView2's hardware compositor tore the picture during
a zoom on some drivers; measured again on a one-hour mix, the tearing was gone
and the GPU was plainly the better of the two. If your driver disagrees, the
other two modes are a launch flag away. Audio is native Rust processing in every
render mode.

| Flag | Render mode |
|---|---|
| *(none)* or --gpu | full GPU rendering (default) |
| --gpu-safe | GPU drawing with software compositing |
| --no-gpu | software rendering |

## Build from source

Requirements: Windows, Node.js with Corepack, the Rust toolchain specified by
rust-toolchain.toml, Microsoft C++ Build Tools with **Desktop development with
C++**, and Microsoft Edge WebView2.

    .\install.cmd     # installs project-local dependencies
    .\dev.cmd         # runs the development build
    .\check.cmd       # TypeScript, frontend tests, Rust tests, fmt, and Clippy

Dependencies and caches remain inside the repository: pnpm does not need to be
installed globally and the project does not modify your PATH. The beat-tracking
models are included, so a fresh clone can analyse music without downloading
anything at first launch.

Close MixCanvas before running check.cmd: Windows locks the bundled ONNX Runtime
DLL while the app is open.

Create a single-file portable build with:

    npx tauri build --no-bundle -f embed-resources

The embed-resources feature packages the interface, analysis models, and ONNX
Runtime into the executable.

## Technology

- Tauri 2, React, TypeScript, and Rust.
- SQLite for the local Library, Timeline, and automation data.
- Symphonia/Rodio for decoding and CPAL for native audio output.
- Beat This! for beat/downbeat analysis and Open-Unmix for stem separation.
- LAME 3.100, compiled in, for MP3 bounces.

For implementation details and design decisions, see
[architecture.md](architecture.md). The architecture notes and source comments
are written in French.

## License

MixCanvas is released under the
[GNU AGPL v3.0 only](LICENSE) (AGPL-3.0-only). You may use, study, share, and
modify it. If you distribute a modified version — including as a network service
— you must make its corresponding source available under the same license.

See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for notices and checksums
for bundled models and binaries.

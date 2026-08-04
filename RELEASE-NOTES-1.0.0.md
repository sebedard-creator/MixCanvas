Build DJ mixes on a timeline instead of on decks.

MixCanvas is a free, open-source Windows application for putting a mix together
before the party: import your MP3s, let it find the beatgrid, arrange clips on a
musical timeline, draw your transitions, and export the result. Everything runs
on your machine — no account, no streaming service, no network request of any
kind.

**Download:** `MixCanvas-1.0.0-portable.exe` — a single file, nothing to install.

### Before you run it

Windows will show **"Windows protected your PC"** the first time. The executable
is not code-signed: a signing certificate is a yearly cost this project does not
carry. Click **More info → Run anyway**.

Verify you have the genuine file if you like:

```powershell
Get-FileHash MixCanvas-1.0.0-portable.exe -Algorithm SHA256
```

```
6f3541bd9f0b40d4dd12f32d7ed87fbf51b3712a2d2378e73d6c633fc8ca6792
```

MixCanvas needs the **Microsoft Edge WebView2 Runtime** to draw its interface.
It ships with Windows 11 and most Windows 10 installs; if yours lacks it, get it
from [Microsoft](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).
You do not need the Edge browser itself.

### Where it puts things

Everything MixCanvas writes goes into a `MixCanvas Files` folder beside the
executable — library, models, stems, baked clips. Copy the two together to move
your setup; delete the folder to start fresh. Your MP3s are never modified and
never moved.

### What's in it

A persistent library with local beat and downbeat detection, a beatgrid editor
for when the analysis needs help, three stereo lanes with pitch-preserving
time-stretch onto a shared tempo curve, volume/pan/filter automation you draw,
a three-band EQ per clip, a master chain with sidechain, glue compression and
limiting, local vocal separation, reversible per-clip bake, portable project
files, and WAV bounce.

Full details in the [README](https://github.com/sebedard-creator/MixCanvas#readme).

Released under the GNU AGPL v3.0.

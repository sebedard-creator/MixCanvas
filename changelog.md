# Changelog

Notable changes to MixCanvas, newest first.

Versions follow [semantic versioning](https://semver.org): the **major** number
moves when a project file or a library written by an older build can no longer
be opened, the **minor** number when something new appears, the **patch** number
when something is fixed.

Entries are written for the person using the program, not for the person who
wrote it. What changed, and what it means when you sit down to build a mix.

---

## 1.0.0 — 2026-08-04

First release.

MixCanvas builds DJ mixes on a timeline rather than on decks: three stereo
tracks, tempo and beatgrid worked out for you, automation you draw, and a mix
that is computed during playback rather than rendered in advance.

What it does is described in the [README](README.md). In short: a persistent
library with local beat detection, pitch-preserving time-stretch onto a shared
tempo curve, per-clip EQ, volume, pan and filter automation, a master chain with
sidechain, glue compression and limiting, local vocal separation, reversible
per-clip bake, portable project files, and WAV bounce.

Everything the program writes lives in one folder beside the executable, and no
part of it makes a network request.

*Development before this release was recorded in a working journal that is not
part of the repository.*

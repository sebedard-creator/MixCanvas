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

The [README](README.md) was rewritten as an English, GitHub-ready product guide:
it presents the complete workflow and feature set without turning the landing
page into internal engineering documentation, including the distinction between
precise automation nodes and a complete Draw gesture. In short: a persistent
library with local beat detection, pitch-preserving time-stretch onto a shared
tempo curve, per-clip EQ, volume, pan and filter automation, a master chain with
sidechain, glue compression and limiting, local vocal separation, reversible
per-clip bake, portable project files, and WAV bounce.

Everything the program writes lives in one folder beside the executable, and no
part of it makes a network request.

The README also names the Microsoft Edge WebView2 Runtime as the Windows display
requirement for portable users, with the official download link and the
clarification that the Edge browser itself is not required.

Four product screenshots now live in `assets/screenshots` and turn the README
into a visual tour: the full Timeline, Beatgrid Editor, Clip EQ, and reversible
Bake workflow.

*Development before this release was recorded in a working journal that is not
part of the repository.*

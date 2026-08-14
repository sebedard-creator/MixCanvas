# Changelog

Notable changes to MixCanvas, newest first.

Versions follow [semantic versioning](https://semver.org): the **major** number
moves when a project file or a library written by an older build can no longer
be opened, the **minor** number when something new appears, the **patch** number
when something is fixed.

Entries are written for the person using the program, not for the person who
wrote it. What changed, and what it means when you sit down to build a mix.

---

## 1.5.1 — 2026-08-14

The four effects move out of their panel and into the console.

### Changed

- **The effect pads live in the top bar now.** `MIX FX` opened a panel over the
  Library column; the pads sit in the console instead, beside the transport,
  five per track. There is nothing to open and nothing to put away, and the
  Library is never covered. They are smaller for it — a flat key rather than a
  square one — because the bar's height is what had to stay fixed, and giving
  them more room would have taken it from the timeline.
- **`AUTO` is a strip along the bottom of `PLAY`.** Autoplay is a rule about
  what a click does to the transport, so it now rides on the transport rather
  than holding a key of its own two blocks away.
- **The transport keys are taller.** They were 54 px, which matched the height
  of the VU lamps but not the VU column. At 68 they match the column, and the
  four groups across the bar come within a few pixels of each other instead of
  eighteen.
- **The VU meter is graduated evenly, in dBFS.** It borrowed the spacing of a
  needle faceplate, where ten decibels were squeezed into the first four lamps
  and three were spread across the last six — 2.31 dB per lamp at the bottom
  against 0.50 at the top. That spacing is for a dial with printed numbers
  under the needle; on a bare row of lamps each one silently meant something
  different from its neighbour. Every lamp is now worth the same 1.67 dB, from
  −40 to 0. The blue-to-amber boundary has not moved in level — it is the same
  −16 dBFS as before — but you can now see how much of the range sits below it.
- **`DRAW` is off unless `VIEW` shows one line.** With both on screen there was
  nothing sensible for the pencil to draw.

### Fixed

- **The top bar lines up again.** Its four groups measured 64, 77, 64 and 82 px
  and were centred on a common axis, so each one floated at its own height and
  none met its neighbour — the 1.0 bar had started them all from a common top
  edge. Evening out the heights fixes what no alignment rule could.
- **The meter shows peaks instead of averaging them away.** Its follower had a
  65 ms attack, which is not a slow peak detector but a different instrument
  altogether: a one-pole filter on |x| settles on the *mean* of the signal, not
  its maximum. A mastered track, whose peaks stand a dozen decibels above its
  mean, could touch full scale while the bar showed two thirds — the meter
  reported a comfortable level while the limiter was working. A peak is now
  taken as it arrives, and the bar falls back at about 10 dB per second.
- **The meter shows the last six decibels before clipping.** Its top mark was
  +3 VU against a reference of 0.35, which is −6.1 dBFS: a full bar and a mix
  about to clip looked exactly the same. The bar is read before the limiter, so
  those are the decibels worth seeing. The top of the scale is full scale now.
- **One drag no longer writes two automations.** With `VIEW` showing volume and
  pan together, a single stroke across a clip wrote a volume shape *and* a pan
  shape — the pointer's height read as decibels for one and as a stereo
  position for the other. Two edits from one gesture, one of them unasked for.

- **Undo no longer keeps a copy of every waveform.** A snapshot carries its
  clips, and a clip carries the drawn peaks of its whole audio; fifty levels of
  undo held fifty copies of the same decoded picture. Measured on a twenty-clip
  session, that is about 15 MiB per level and roughly 0.7 GiB across the
  history. The peaks are dropped on the way into the history now — restoring
  never read them, and the timeline redraws from the snapshot the backend
  returns, which is re-read from the database.

### Removed

- **`REW` and `FFWD`.** They were added to the effects panel because leaving it
  to replay a passage broke the thread. The pads are next to the transport now,
  so the problem they solved no longer exists.

## 1.5.0 — 2026-08-13

Mix Effects: four effects you play by hand onto a track, and they write
themselves onto the timeline.

### Added

- **Mix Effects: play four effects onto a track by hand.** A `MIX FX` button
  toggles a panel that sits over the library column rather than covering the
  timeline — square icon pads, four per track, drawn like the transport keys.
  Press `MIX FX` again to put it away. Hold reverb for a shared room, flange
  for a sweeping comb, crush to drop the track to eight bits, or delay for an
  echo. Reverb, flange and delay keep ringing after you let go, because they
  sit on sends — hold the delay, cut the track, and the echo
  carries the transition. Crush replaces the sound only while you hold it,
  because it sits inline; the other three are taken after it, so you hear the
  room around the crushed sound.
- **The delay follows the project tempo.** A dotted-eighth echo, recomputed from
  the tempo map on every frame, so it stays on the beat even through a tempo
  ramp — where an echo set in milliseconds would drift off it. It ping-pongs
  across the ears — first repeat left, second right, third left — and each
  repeat comes back a little darker, so it sits behind the music instead of
  fighting it.
- **A played pass is written onto the timeline.** Release the pad and the pass
  becomes automation you can keep: a coloured region on the track — purple for
  reverb, green for flange, magenta for crush, orange for delay — whose shading
  is the automation curve itself. It fades in and out exactly where the ramp
  does, on a long pass and a short one alike. Where effects overlap on one
  track, the region is hatched in their colours rather than blended into one
  that means none of them. `Ctrl+Z` undoes a pass like any other edit, and
  replaying over a region corrects it instead of stacking a second one.
  Right-click a region for **Delete Mix FX Automation**, which clears every
  effect under the cursor in one undoable step.
- **An eraser you play like an effect.** A last pad on each track, held the
  same way: sweep it over automation while the music runs and it wipes every
  effect it passes, in one undoable step. The track falls silent under the
  eraser as you sweep, so you hear what you are removing while you remove it
  rather than after you let go. To remove just one effect, right-click its
  region on the timeline instead.
- **The pass draws itself while you play it.** A coloured band grows from where
  you pressed and follows the playhead, so you can see how far you have taken
  it. The finished region, with its fades, takes over the moment you let go.
- **The pad follows the timeline, not just your finger.** When a recorded pass
  plays back under the playhead, its pad lights up the same way it does when
  held — the panel tells you what you are hearing.
- **`REW` and `FFWD` on the Mix Effects panel.** Replaying a pass you just
  missed is the most common thing to do here, and leaving the panel to do it
  broke the thread. A click jumps one bar — musical rather than a number of
  seconds — and **holding `FFWD` fast-forwards at 2×**.

### Changed

- **The library remembers how you sorted it.** It went back to sorting by artist
  every time the program started. Your choice is now kept beside the executable,
  with everything else the program writes.
- **The delay carries.** Seven repeats, each quieter than the last so the echo
  tails off on its own, and the loudest return of the four — a repeat is a
  single event with one moment to be heard, where a reverb tail has seconds.
- **Crush sits under the track instead of on top of it.** It was the only one of
  the four running at full level — the others are all mixed in under the dry
  sound — so it came across louder than its neighbours. It is 3 dB quieter now.
- **The reverb has more air, and sits further back.** It was missing high end:
  the tail now keeps its highs far longer, and the return is lifted about 3 dB
  above 3 kHz, with a ceiling at 12 kHz so the lift does not turn into hiss.
  The return itself is 6 dB quieter to make room for it — brighter, and sitting
  further behind the music than it used to.

### Fixed

- **Effect tails last the same time on any audio device.** Their budgets were
  written in frames at 48 kHz, so on a 96 kHz interface they were cut in half —
  the delay's echo would have stopped dead partway through on a slow track.
- **The Beatgrid Editor's `Save` waits for `Snap to beat` to finish.** Snapping
  rewrites the tempo and downbeat when it lands, so saving mid-snap quietly kept
  the values from before it ran. `Restore Automatic` and `Reanalyze` wait for
  the same reason.
- **Clicking the timeline moves the playhead again.** The coloured effect
  regions were catching the click meant for the track underneath, so once you
  had played a few passes much of the timeline stopped responding. A tint is a
  background now, not a control; right-click still removes the region under the
  cursor.
- **Reverb and delay fade away when you let go instead of stopping dead.**
  Releasing a pad writes the pass, and writing used to rebuild the mix from
  scratch — which emptied the room and the echo line at the exact moment their
  tail should have started.
- **A pad lights only while its effect is sounding.** It used to keep a coloured
  mark once you had used it during the session — a second meaning on the same
  button, and one that never went away. A pad and its coloured region on the
  timeline now say the same thing at the same moment.
- **A pad no longer stays lit after you stop.** The transport is only polled
  during playback, so pausing inside a recorded pass left its pad on with
  nothing left to turn it off — the reverb pad in particular looked stuck on.
  A pad shows what you are hearing, and stopped you hear nothing.
- **A held pad can always be released.** If an edit started while you were
  holding one, the pad went disabled mid-gesture and never saw your finger lift,
  leaving the effect open.
- **`Ctrl+Z` now undoes a played effect pass.** It undid every other kind of
  edit, but a reverb pass stayed on the timeline and kept playing.
- **`CLEAR TIMELINE` now clears played effect automation too.** It was left
  behind with no clip to carry it — invisible, since there was nothing to tint —
  and came back audible as soon as you dropped a clip in the same place.

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

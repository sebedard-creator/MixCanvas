# Changelog

Notable changes to MixCanvas, newest first.

Versions follow [semantic versioning](https://semver.org): the **major** number
moves when a project file or a library written by an older build can no longer
be opened, the **minor** number when something new appears, the **patch** number
when something is fixed.

Entries are written for the person using the program, not for the person who
wrote it. What changed, and what it means when you sit down to build a mix.

---

## 1.7.1 — 2026-08-20

### Removed

- **The tempo field's up and down arrows.** Right-clicking the ruler to type a
  clip's tempo opened a number field, and a number field brings its own pair of
  spinners. Their step was the field's own: one thousandth of a beat, so a
  thousand presses moved the tempo by a single BPM. Nobody has ever set a tempo
  that way; they existed to be clicked by accident, and clicking one could
  blank the timeline. The field is plain text now, so there are no arrows to
  hide and none left answering the keyboard in secret. The 40–300 range was
  never enforced by the field anyway — it lives in the code that applies the
  value, and still does. The same spinners are gone from the Beatgrid Editor's
  Source BPM.

---

## 1.7.0 — 2026-08-18

### Added

- **Duplicate a clip from the right-click menu.** It copies the clip exactly as
  it is trimmed — cut four bars out of a six-minute track and you get four bars,
  needing four bars of room. The copy never displaces anything. It looks for a
  free track at the same beats first, since that costs the mix no space at all;
  failing that it goes immediately to the right of the clip, then immediately to
  the left, always landing on a bar line so you can still drag it. If none of
  the four places is clear, it says so rather than pushing something aside.
  What belongs to the clip comes along: its EQ, its trim, the voice it plays,
  its separated stems and its bake. What belongs to the lane stays where it is —
  volume, pan, filter curves and recorded effect passes were drawn at a place on
  a track, not onto the clip, and carrying them would write over whatever is
  already there.

- **Loopable clips.** A new `∞` key on each clip, between `MUS` and the
  sidechain. Turn it on and the clip's two edges stop trimming and start
  repeating it — drag either one out and the pattern fills the space, on the
  beat, for as long as you pull. It is a state, not an action: turning it off
  leaves the clip exactly as it was, because the repeated pattern is still
  described by the trim. The last turn at each end stops where you let go
  rather than at the next repeat, and a thin line marks every seam so an
  eight-bar loop does not read as one long clip. A loop refuses to run into the
  clip after it, the same way moving one does — the room is there or it is not.
- **Mute one clip.** A small `M` between `BAKE` and the close button, red while
  it is on. The lane `MUTE` answers a different question — *I do not want to
  hear this track* — and takes the clips beside it with it. This one silences
  the clip you clicked and nothing else. It is not a deletion: the EQ, the
  automation drawn over it and the bake all stay put and come back untouched,
  which is what makes it a gesture you can afford mid-mix. A muted clip stays
  out of the bounce too, and still counts towards the length of the project.

### Fixed

- **The Clip EQ window stops blinking on some graphics drivers.** Its backdrop
  was blurred, which asks the compositor to re-read everything underneath on
  every frame. It was the only blurred surface in the program sitting over
  *moving* content — the Beatgrid Editor and the help window are read while
  playback is stopped, while the EQ stays open as the timeline scrolls behind
  it. On an older card in full-GPU mode that produced a one-frame blank; both
  other render modes were clean. The blur is gone and the veil behind the
  window is darker instead, which separates it just as well and asks nothing of
  the compositor.
- **The mastering limiter's default threshold catches up with `COMP`.**
  Softening the compressor's smiling V took level out of the colour stage, so
  the mix reached the limiter a little quieter than before for the same result.
  Measured across the two curves weighted by a programme spectrum: 0.40 dB less
  — 0.28 dB weighted as white noise. The default threshold moves from −3.7 to
  −4.0 dB, which sits inside that band and stays a number you can read. The
  ceiling does not move, so an armed bounce lifts 3.9 dB instead of 3.6.
- **Looped waveforms stop dancing during playback.** A clip's waveform bitmap
  was as many pixels wide as it had columns of data, which is right for a whole
  clip — the pyramid level is picked to give one column per pixel. A short
  slice breaks that: four beats taken from a six-minute track keep only
  ninety-one columns, stretched by CSS across a hundred and sixty pixels. The
  browser resamples the picture, and because the timeline scrolls by fractions
  of a pixel while playing, the resampling phase changes every frame. One clip
  shimmers unnoticeably; a row of loop turns, each at its own fraction,
  shimmers independently — which reads as dancing. The bitmap is now made at
  the size it will actually occupy, and each turn is placed on whole pixels,
  with its width taken from the next turn's edge so no seam gains or loses a
  pixel.
- **Volume envelopes follow the ear instead of the ruler.** The lower half of
  a lane spread forty decibels evenly down the travel. That is regular to the
  eye and wrong to it: half way down was −20 dB — a tenth of the amplitude,
  where you expect *about half as loud* — while a tenth of the travel already
  cost four decibels, so fine adjustment near unity was impossible, and the
  bottom quarter ran from −30 to −40 dB where nothing is audible in a mix
  anyway. The travel is squared now, which is roughly what a console fader
  does: half way down is −10 dB, three quarters down is −22.5, and a tenth of
  the travel costs 0.4 dB instead of 4 — ten times finer where the work
  actually happens. Only the drawing changes. The engine still receives
  decibels, so a saved project sounds exactly the same; its nodes are simply
  drawn higher up the lane than before.
- **The Clip EQ stops stuttering, and stops eating the undo history.** Every
  live save while dragging a slider went through the same path as a real edit:
  it pushed a new timeline into React — re-rendering the whole timeline panel,
  the heaviest thing in the program — and added an entry to the undo history.
  Five times a second. The slider stuttered at exactly that rhythm, and three
  seconds of tweaking burned fifteen of the fifty undo levels. Neither was
  needed: the live save exists only so the engine hears the change, and the
  window covers the timeline anyway. The timeline is now read once, when the
  window closes, with the single undo entry that belongs to the whole
  adjustment.
- **A bar cut by hand now loops without a gap.** Splitting happened at the exact
  position of the playhead, which is never on a round beat: what looked like one
  bar was 4.0173 beats long. Clip anchors are whole beats and land on bar lines,
  so no position on the grid could follow such a clip end to end — duplicating
  it left four beats of silence, and dragging the copy back to close the gap was
  refused because it overlapped the original by seventeen thousandths of a beat.
  A split now lands on the nearest whole beat. The rule lives in the engine
  rather than the interface, so the `B` key and the menu cut in the same place.
- **The tempo box no longer opens off-screen.** Right-clicking the BPM of the
  first clip in a mix put the box on the node it edits and centred it there,
  which is right everywhere except a few pixels from the edge: half of it went
  past the left of the window and the field could not be seen. It now slides
  back inside, at either end.
- **`+ MP3` stops claiming an import that is not happening.** The button read
  *Importing…* whenever it was unavailable, and one of the reasons it becomes
  unavailable is that the mix is playing. It keeps its name now and simply
  greys out; hovering either it or `Add Folder` says which of the three reasons
  it is — an import in progress, the Beatgrid Editor being open, or the mix
  playing.

### Changed

- **The eraser pad is drawn again.** It was a school eraser tilted thirty-two
  degrees, and at twelve pixels the block and its band collapsed into one
  leaning smudge. It is a proper forty-five degrees now, resting on the sheet,
  with the working corner worn flat — at exactly that angle the wear facet
  comes out horizontal, so it reads as use rather than as a slip of the pen.
  The line under it is intact ahead of the eraser and gone behind, which is the
  only part of the drawing that says which way the gesture runs.

---

## 1.6.0 — 2026-08-16

### Fixed

- **Zooming keeps the timeline in one piece.** Past the first few minutes of a
  long mix, a zoom step could leave the clips, waveforms and automation sitting
  at a different position than the grid and the playhead — barely a jitter near
  the start, unwatchable an hour in. The view was being placed by writing a
  scroll position, and a browser silently trims that request to the width it
  currently believes the content has. Right after a zoom, that belief is one
  step out of date, so the timeline landed at the previous step's limit instead
  of where it was asked to go. The further along the mix, the wider the gap.
  The visible surface is now positioned directly rather than requested, which
  has no such dependency: every element lands on the same beat at every zoom
  level. Getting there also removed a real amount of work from each step —
  filter samples are sorted only when their musical data changes, and a clip's
  visible waveform is cached as a bitmap that a zoom step resizes instead of
  making the browser retessellate four long paths per clip.

  A single blank frame can still appear as the timeline redraws at the new
  scale. Hardware rendering, now the default, makes it far less noticeable.

- **Waveforms line up with the sound when you zoom right out.** A clip's
  waveform was drawn into a width sixteen pixels narrower than the clip itself,
  so the picture ran on a slightly different time axis. On a wide clip the
  error was under two per cent and invisible; zoomed far out, where a whole
  track is a hundred pixels across, it was sixteen per cent — the sound kept
  going for several bars after the drawing had ended. The waveform now spans
  the clip exactly. The eight-pixel inset it used to have was cosmetic, and
  horizontal space is time.

### Changed

- **`COMP` colours with a lighter hand.** Its console tilt was two shelves of
  +2 dB, one under 90 Hz and one over 10 kHz. A shelf keeps climbing to the
  edge of hearing and never comes back down, so the top one was lifting
  everything above 10 kHz by the same two decibels — cymbals, sibilance and
  hiss together. Checked on other systems, the smile was too wide at both ends.
  The low shelf is now +1.5 dB, and the top is a bell centred at 13 kHz worth
  +1.0 dB: it opens, and it closes. From 18 kHz up the curve is flat, because
  nobody in the room hears what is up there and the encoder should not spend
  bits on it. Measured against the old curve: −0.5 dB at 50 Hz, −1.0 dB at
  10 kHz, −1.6 dB at 16 kHz, −2.0 dB from 18 kHz up. Nothing changes when
  `COMP` is off.
- **The portable build now draws with the GPU by default.** Software drawing
  had that job since July, because WebView2's hardware compositor tore the
  picture during a zoom on some drivers. Fixing the zoom geometry changed what
  was left to see: with everything finally in step, the remaining fault was a
  blank frame while the processor repainted a long surface by itself — and the
  graphics card does that without breaking a sweat. Retested on a one-hour mix,
  the tearing did not come back and the GPU was plainly the better of the two.
  The other two modes are still one launch flag away, because this kind of
  fault belongs to the driver more than to the program: start with `--no-gpu`
  for the old software behaviour, or `--gpu-safe` for hardware drawing with
  software compositing.

### Added

- **A mastering limiter on the bounce.** `BOUNCE MIX` now opens a short dialog
  with a **Mastering Limiter**, armed unless you turn it off — a mix rendered
  without one is quieter than everything it will be played next to, and that is
  the kind of fault nobody notices, because it does not make a noise, it makes
  less of one. It is a different instrument from the
  one on the transport: that one is a safeguard for listening, two milliseconds
  of attack and a hard clamp at the end. This one looks three milliseconds
  ahead, so the gain is already down when the peak arrives — nothing gets past
  the ceiling, and transients are limited rather than clipped. Lowering the
  threshold lifts the whole mix to the ceiling, the way a mastering limiter is
  expected to, and the dialog says by how many decibels so the gain is never a
  surprise. Release can follow the programme: quick after a lone peak, slow
  through a dense passage, so it stops pumping on the kick. Arming it renders
  without the safeguard, whose −0.18 dBFS clamp would otherwise cut the
  transients before this limiter ever saw them. The settings are remembered
  between bounces.
- **Bounce to MP3.** The dialog now offers 44.1 kHz CBR 320 kbps stereo
  alongside the WAV, encoded with LAME at `q0` — the most thorough
  psychoacoustic search it offers, which costs time an offline render has to
  spare. The mix reaches the encoder in floating point and is never quantised
  to 16 bits on the way, so there is no dither noise for the encoder to spend
  bits preserving. Plain stereo rather than the joint stereo LAME would pick on
  its own: at 320 kbps nothing forces the two channels to share, and the played
  effects work the stereo image. WAV stays the default — a master is kept
  lossless, and the mistake only goes one way.
- **The ceiling makes room for MP3 on its own.** A lossy codec does not
  rebuild peaks exactly: measured on a sixty-three minute mix, the decoded MP3
  reached +0.385 dB where the WAV stopped dead at the limiter's ceiling. A
  player that clips at zero shaves those, and it is audible. Picking MP3 now
  sets the ceiling to −1.0 dB — no extra control, the field that is already
  there simply carries the value the format needs, and it stays editable.
- **Shift a downbeat from the right-click menu.** Analysis sometimes lands the
  first beat on the 2 or the 3 of the bar: the grid is right, it is only
  turned. Right-click a track in the Library, or a clip on the timeline, and
  nudge the downbeat a beat either way. The tempo does not move. The correction
  belongs to the track, so every clip of it follows — and a shift back onto the
  analysed value clears the correction instead of recording it.

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

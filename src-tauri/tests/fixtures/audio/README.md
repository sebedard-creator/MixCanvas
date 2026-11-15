# Audio format fixtures

Synthetic signals generated with FFmpeg; no third-party recordings. FFmpeg is
needed only to regenerate these files. Tests read the checked-in fixtures.

| File | Encoding | Rate | Channels | Duration |
| --- | --- | --- | --- | --- |
| pcm24-stereo.wav | signed PCM, 24 bits | 48 kHz | 2 | 0.25 s |
| float32-mono.wav | floating PCM, 32 bits | 96 kHz | 1 | 0.125 s |
| tagged-stereo.flac | FLAC, 24 bits | 48 kHz | 2 | 0.25 s |
| tagged-stereo.mp3 | LAME VBR | 44.1 kHz | 2 | 2 s |
| clicks-120bpm.flac | FLAC, 16 bits | 44.1 kHz | 1 | 16 s |

Run from this directory in PowerShell:

```powershell
ffmpeg -y -v error -f lavfi -i 'sine=frequency=440:sample_rate=48000:duration=0.25' -af 'pan=stereo|c0=c0|c1=0.5*c0' -c:a pcm_s24le -metadata 'artist=MixCanvas Test' -metadata 'title=PCM 24' pcm24-stereo.wav
ffmpeg -y -v error -f lavfi -i 'sine=frequency=660:sample_rate=96000:duration=0.125' -c:a pcm_f32le float32-mono.wav
ffmpeg -y -v error -f lavfi -i 'sine=frequency=440:sample_rate=48000:duration=0.25' -af 'pan=stereo|c0=c0|c1=0.5*c0' -c:a flac -sample_fmt s32 -compression_level 8 -metadata 'artist=Équipe MixCanvas' -metadata 'title=FLAC Test' tagged-stereo.flac
ffmpeg -y -v error -f lavfi -i 'sine=frequency=440:sample_rate=44100:duration=2' -ac 2 -c:a libmp3lame -q:a 2 -metadata 'artist=MixCanvas Test' -metadata 'title=MP3 Test' tagged-stereo.mp3
ffmpeg -y -v error -f lavfi -i 'aevalsrc=if(lt(mod(t\,0.5)\,0.09)\,0.7*sin(2*PI*65*t)*exp(-mod(t\,0.5)*50)\,0):s=44100:d=16' -c:a flac -sample_fmt s16 -compression_level 8 clicks-120bpm.flac
```

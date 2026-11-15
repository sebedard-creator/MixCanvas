use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    time::Duration,
};

use rodio::{Decoder, Source};
use symphonia::core::{
    io::MediaSourceStream,
    meta::{Limit, MetadataOptions, MetadataRevision, StandardTagKey},
    probe::Hint,
};

pub(crate) type AudioDecoder = Decoder<BufReader<File>>;

#[derive(Clone, Debug)]
pub(crate) struct AudioFileMetadata {
    pub duration: Duration,
    pub sample_rate: u32,
    pub channels: u16,
    pub artist: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AudioTags {
    pub artist: Option<String>,
    pub title: Option<String>,
}

pub(crate) fn open_audio_decoder(path: &Path) -> Result<AudioDecoder, String> {
    validate_audio_path(path)?;

    let file =
        File::open(path).map_err(|error| format!("Could not open the audio file: {error}"))?;
    Decoder::try_from(file).map_err(|error| format!("Could not decode the audio file: {error}"))
}

pub(crate) fn metadata_from_decoder(decoder: &AudioDecoder) -> AudioFileMetadata {
    AudioFileMetadata {
        duration: decoder.total_duration().unwrap_or_default(),
        sample_rate: decoder.sample_rate().get(),
        channels: decoder.channels().get(),
        artist: None,
        title: None,
    }
}

pub(crate) fn inspect_audio(path: &Path) -> Result<AudioFileMetadata, String> {
    let decoder = open_audio_decoder(path)?;
    let mut metadata = metadata_from_decoder(&decoder);
    let tags = read_audio_tags(path);
    metadata.artist = tags.artist;
    metadata.title = tags.title;
    Ok(metadata)
}

pub(crate) fn read_audio_tags(path: &Path) -> AudioTags {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp3"))
    {
        return read_mp3_id3_tags(path);
    }
    read_container_tags(path).unwrap_or_default()
}

fn read_container_tags(path: &Path) -> Option<AudioTags> {
    if !is_supported_audio_path(path) {
        return None;
    }
    let file = File::open(path).ok()?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }
    let options = MetadataOptions {
        limit_metadata_bytes: Limit::Maximum(16 * 1024 * 1024),
        limit_visual_bytes: Limit::Maximum(0),
    };
    let mut probed = symphonia::default::get_probe()
        .format(&hint, source, &Default::default(), &options)
        .ok()?;
    let mut tags = AudioTags::default();
    if let Some(mut metadata) = probed.metadata.get() {
        merge_container_tags(&mut tags, metadata.skip_to_latest());
    }
    merge_container_tags(&mut tags, probed.format.metadata().skip_to_latest());
    Some(tags)
}

fn merge_container_tags(tags: &mut AudioTags, revision: Option<&MetadataRevision>) {
    let Some(revision) = revision else { return };
    for tag in revision.tags() {
        match tag.std_key {
            Some(StandardTagKey::Artist) if tags.artist.is_none() => {
                tags.artist = clean_tag(tag.value.to_string());
            }
            Some(StandardTagKey::TrackTitle) if tags.title.is_none() => {
                tags.title = clean_tag(tag.value.to_string());
            }
            _ => {}
        }
    }
}

fn read_mp3_id3_tags(path: &Path) -> AudioTags {
    let Ok(mut file) = File::open(path) else {
        return AudioTags::default();
    };
    let v2 = read_id3v2_tags(&mut file).unwrap_or_default();
    let v1 = read_id3v1_tags(&mut file).unwrap_or_default();
    AudioTags {
        artist: v2.artist.or(v1.artist),
        title: v2.title.or(v1.title),
    }
}

fn read_id3v2_tags(file: &mut File) -> Option<AudioTags> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut header = [0_u8; 10];
    file.read_exact(&mut header).ok()?;
    if &header[..3] != b"ID3" || !(2..=4).contains(&header[3]) {
        return None;
    }
    let tag_size = synchsafe_u32(&header[6..10])? as usize;
    if tag_size > 16 * 1024 * 1024 {
        return None;
    }
    let mut body = vec![0_u8; tag_size];
    file.read_exact(&mut body).ok()?;
    if header[5] & 0x80 != 0 {
        body = remove_unsynchronisation(&body);
    }
    Some(parse_id3v2_frames(header[3], &body))
}

fn read_id3v1_tags(file: &mut File) -> Option<AudioTags> {
    if file.seek(SeekFrom::End(-128)).is_err() {
        return None;
    }
    let mut tag = [0_u8; 128];
    file.read_exact(&mut tag).ok()?;
    if &tag[..3] != b"TAG" {
        return None;
    }
    Some(AudioTags {
        title: decode_latin1(&tag[3..33]),
        artist: decode_latin1(&tag[33..63]),
    })
}

fn parse_id3v2_frames(version: u8, body: &[u8]) -> AudioTags {
    let mut tags = AudioTags::default();
    let mut offset = 0_usize;
    let header_size = if version == 2 { 6 } else { 10 };

    while offset.saturating_add(header_size) <= body.len() {
        let header = &body[offset..offset + header_size];
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let (id, frame_size) = if version == 2 {
            let Ok(id) = std::str::from_utf8(&header[..3]) else {
                break;
            };
            let size = (usize::from(header[3]) << 16)
                | (usize::from(header[4]) << 8)
                | usize::from(header[5]);
            (id, size)
        } else {
            let Ok(id) = std::str::from_utf8(&header[..4]) else {
                break;
            };
            let size = if version == 4 {
                synchsafe_u32(&header[4..8]).unwrap_or_default() as usize
            } else {
                u32::from_be_bytes(header[4..8].try_into().unwrap_or_default()) as usize
            };
            (id, size)
        };
        offset = offset.saturating_add(header_size);
        let Some(end) = offset.checked_add(frame_size) else {
            break;
        };
        if frame_size == 0 || end > body.len() {
            break;
        }
        let value = decode_id3_text(&body[offset..end]);
        match id {
            "TPE1" | "TP1" if tags.artist.is_none() => tags.artist = value,
            "TIT2" | "TT2" if tags.title.is_none() => tags.title = value,
            _ => {}
        }
        if tags.artist.is_some() && tags.title.is_some() {
            break;
        }
        offset = end;
    }
    tags
}

fn synchsafe_u32(bytes: &[u8]) -> Option<u32> {
    let [a, b, c, d] = bytes.try_into().ok()?;
    if [a, b, c, d].iter().any(|byte| byte & 0x80 != 0) {
        return None;
    }
    Some((u32::from(a) << 21) | (u32::from(b) << 14) | (u32::from(c) << 7) | u32::from(d))
}

fn decode_id3_text(bytes: &[u8]) -> Option<String> {
    let (&encoding, text) = bytes.split_first()?;
    match encoding {
        0 => decode_latin1(text),
        1 => decode_utf16(text, None),
        2 => decode_utf16(text, Some(true)),
        3 => std::str::from_utf8(text)
            .ok()
            .and_then(|value| clean_tag(value.to_owned())),
        _ => None,
    }
}

fn decode_latin1(bytes: &[u8]) -> Option<String> {
    clean_tag(bytes.iter().map(|byte| char::from(*byte)).collect())
}

fn decode_utf16(bytes: &[u8], forced_big_endian: Option<bool>) -> Option<String> {
    let (big_endian, bytes) = match (forced_big_endian, bytes) {
        (Some(big_endian), bytes) => (big_endian, bytes),
        (None, [0xFE, 0xFF, rest @ ..]) => (true, rest),
        (None, [0xFF, 0xFE, rest @ ..]) => (false, rest),
        (None, bytes) => (false, bytes),
    };
    let units = bytes
        .chunks_exact(2)
        .map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        })
        .take_while(|unit| *unit != 0);
    String::from_utf16(&units.collect::<Vec<_>>())
        .ok()
        .and_then(clean_tag)
}

fn clean_tag(value: String) -> Option<String> {
    let value =
        value.trim_matches(|character: char| character == '\0' || character.is_whitespace());
    (!value.is_empty()).then(|| value.to_owned())
}

fn remove_unsynchronisation(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        result.push(bytes[index]);
        if bytes[index] == 0xFF && bytes.get(index + 1) == Some(&0) {
            index += 1;
        }
        index += 1;
    }
    result
}

/// Formats acceptés à l'import et par tous les chemins de décodage.
pub(crate) const SUPPORTED_AUDIO_EXTENSIONS: [&str; 3] = ["mp3", "wav", "flac"];

pub(crate) fn is_supported_audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SUPPORTED_AUDIO_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

fn validate_audio_path(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("The selected file does not exist, or cannot be read.".to_owned());
    }
    if !is_supported_audio_path(path) {
        return Err("Supported audio formats are MP3, WAV and FLAC.".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AudioTags, inspect_audio, is_supported_audio_path, open_audio_decoder, parse_id3v2_frames,
        validate_audio_path,
    };
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn rejects_a_path_that_is_not_a_file() {
        let error = validate_audio_path(Path::new("a-file-that-does-not-exist.mp3"));

        assert!(error.is_err());
    }

    #[test]
    fn imports_decode_and_seek_mp3_wav_and_flac_at_their_native_rates() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio");
        for (name, rate, channels, seconds) in [
            ("pcm24-stereo.wav", 48_000, 2, 0.25),
            ("float32-mono.wav", 96_000, 1, 0.125),
            ("tagged-stereo.flac", 48_000, 2, 0.25),
            ("tagged-stereo.mp3", 44_100, 2, 2.0),
        ] {
            let path = fixtures.join(name);
            let metadata = inspect_audio(&path).unwrap();
            assert_eq!(metadata.sample_rate, rate, "{name}");
            assert_eq!(metadata.channels, channels, "{name}");
            assert!(
                (metadata.duration.as_secs_f64() - seconds).abs() < 0.04,
                "{name}: {:?}",
                metadata.duration
            );
            let mut decoder = open_audio_decoder(&path).unwrap();
            let energy: f32 = decoder
                .by_ref()
                .take(1024)
                .map(|sample| sample * sample)
                .sum();
            assert!(energy > 0.1, "{name} must decode real samples");
            rodio::Source::try_seek(&mut decoder, Duration::from_secs_f64(seconds / 2.0)).unwrap();
            let energy: f32 = decoder.take(4096).map(|sample| sample * sample).sum();
            assert!(energy > 0.1, "{name} must remain audible after a seek");
            let waveform = crate::analysis::analyze_waveform(&path).unwrap();
            assert!(
                waveform.left_max.iter().any(|peak| *peak > 0.05),
                "{name} waveform"
            );
        }
    }

    #[test]
    fn keeps_id3_and_reads_riff_and_vorbis_artist_title_tags() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio");
        for (name, artist, title) in [
            ("pcm24-stereo.wav", "MixCanvas Test", "PCM 24"),
            ("tagged-stereo.flac", "Équipe MixCanvas", "FLAC Test"),
            ("tagged-stereo.mp3", "MixCanvas Test", "MP3 Test"),
        ] {
            let metadata = inspect_audio(&fixtures.join(name)).unwrap();
            assert_eq!(metadata.artist.as_deref(), Some(artist), "{name}");
            assert_eq!(metadata.title.as_deref(), Some(title), "{name}");
        }
    }

    #[test]
    fn accepts_uppercase_extensions_and_keeps_the_format_boundary() {
        for name in ["TRACK.MP3", "TRACK.WaV", "TRACK.FLAC"] {
            assert!(is_supported_audio_path(Path::new(name)));
        }
        for name in ["track.aac", "track.flac.exe", "track", "notes.txt"] {
            assert!(!is_supported_audio_path(Path::new(name)));
        }
    }

    #[test]
    fn reads_artist_and_title_from_id3v23_frames() {
        let mut body = Vec::new();
        body.extend_from_slice(b"TPE1");
        body.extend_from_slice(&[0, 0, 0, 7]);
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&[3, b'B', b'i', b'c', b'e', b'p', 0]);
        body.extend_from_slice(b"TIT2");
        body.extend_from_slice(&[0, 0, 0, 6]);
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&[3, b'G', b'l', b'u', b'e', 0]);

        assert_eq!(
            parse_id3v2_frames(3, &body),
            AudioTags {
                artist: Some("Bicep".to_owned()),
                title: Some("Glue".to_owned()),
            }
        );
    }

    #[test]
    fn reads_id3v22_latin1_frames() {
        let mut body = Vec::new();
        body.extend_from_slice(b"TP1");
        body.extend_from_slice(&[0, 0, 6]);
        body.extend_from_slice(&[0, b'A', b'p', b'h', b'e', b'x']);
        body.extend_from_slice(b"TT2");
        body.extend_from_slice(&[0, 0, 5]);
        body.extend_from_slice(&[0, b'X', b't', b'a', b'l']);

        assert_eq!(
            parse_id3v2_frames(2, &body),
            AudioTags {
                artist: Some("Aphex".to_owned()),
                title: Some("Xtal".to_owned()),
            }
        );
    }
}

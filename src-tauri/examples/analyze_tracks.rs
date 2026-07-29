use std::{
    env,
    path::{Path, PathBuf},
};

use mixcanvas_lib::analysis::{BeatModelPaths, analyze_mp3};

fn main() {
    let mut arguments = env::args().skip(1);
    let Some(resources) = arguments.next().map(PathBuf::from) else {
        eprintln!(
            "Usage: cargo run --example analyze_tracks -- <resources-folder> <track.mp3> [...]"
        );
        std::process::exit(2);
    };
    let paths = arguments.collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!(
            "Usage: cargo run --example analyze_tracks -- <resources-folder> <track.mp3> [...]"
        );
        std::process::exit(2);
    }
    let models = BeatModelPaths {
        mel: resources.join("models").join("mel_spectrogram.onnx"),
        beats: resources.join("models").join("beat_this_small.onnx"),
    };

    let mut failed = false;
    for path in paths {
        let started = std::time::Instant::now();
        match analyze_mp3(Path::new(&path), &models) {
            Ok(analysis) => println!(
                "{}\t{:.3} BPM\tconfidence {:.3}\tfirst downbeat {} ms\t{} beats\t{:.2} s",
                path,
                analysis.bpm,
                analysis.confidence,
                analysis.first_beat_ms,
                analysis.beats_ms.len(),
                started.elapsed().as_secs_f64(),
            ),
            Err(error) => {
                eprintln!("{path}\tERROR\t{error}");
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}

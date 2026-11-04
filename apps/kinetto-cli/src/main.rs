//! Command-line Kinetto: inspect a project or turn it into a video without
//! opening the editor.

use kn_engine::storage;
use kn_export::ExportSettings;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

const USAGE: &str = "usage:
  kinetto-cli sample <project.kinetto>
  kinetto-cli info <project.kinetto>
  kinetto-cli export <project.kinetto> <video.mp4> [--music calm|upbeat|epic]
  kinetto-cli music <calm|upbeat|epic> <seconds> <out.wav>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["sample", out] => sample(out.into()),
        ["info", project] => info(project.into()),
        ["export", project, out] => export(project.into(), out.into(), None),
        ["export", project, out, "--music", mood] => mood_named(mood).and_then(|m| export(project.into(), out.into(), Some(m))),
        ["music", mood, secs, out] => mood_named(mood).and_then(|m| music(m, secs, out.into())),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn load_error(path: &std::path::Path, e: &storage::StorageError) -> String {
    let why = match e {
        storage::StorageError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => "no such file".into(),
        storage::StorageError::Io(io) => io_hint(io),
        other => other.to_string(),
    };
    format!("couldn't open {}: {why}", path.display())
}

/// What a file error means, and what to do about it.
fn io_hint(e: &std::io::Error) -> String {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound => "that folder doesn't exist; create it or choose another".into(),
        K::PermissionDenied => "no permission to write there; choose a folder you own".into(),
        K::StorageFull | K::QuotaExceeded => "the disk is full; free some space or choose another drive".into(),
        K::ReadOnlyFilesystem => "that drive is read-only; choose another folder".into(),
        _ => e.to_string(),
    }
}

/// Writes the starter deck, handy for trying export.
fn sample(out: PathBuf) -> Result<(), String> {
    let project = kn_model::Project::sample();
    storage::save(&project, &kn_media::MediaLibrary::new(), &out).map_err(|e| e.to_string())
}

fn info(path: PathBuf) -> Result<(), String> {
    let bundle = storage::load(&path).map_err(|e| load_error(&path, &e))?;
    let p = &bundle.project;
    println!("slides:   {}", p.slides.len());
    println!("length:   {:.1} s", p.total_duration().as_secs_f64());
    println!("shape:    {:?}", p.aspect);
    println!("pictures: {}", bundle.media.ids().count());
    Ok(())
}

fn mood_named(name: &str) -> Result<kn_model::Mood, String> {
    kn_model::Mood::ALL.into_iter().find(|m| m.name().eq_ignore_ascii_case(name)).ok_or_else(|| format!("unknown mood {name}\n{USAGE}"))
}

/// Saves generated music, to hear a mood on its own.
fn music(mood: kn_model::Mood, secs: &str, out: PathBuf) -> Result<(), String> {
    let secs: f64 = secs.parse().map_err(|_| format!("not a number of seconds: {secs}"))?;
    let mut pcm = kn_audio::generate(mood, (secs.clamp(1.0, 600.0) * f64::from(kn_audio::RATE)) as usize);
    kn_audio::loudness::normalise(&mut pcm.samples);
    std::fs::write(&out, pcm.to_wav()).map_err(|e| e.to_string())
}

fn export(path: PathBuf, out: PathBuf, mood: Option<kn_model::Mood>) -> Result<(), String> {
    let mut bundle = storage::load(&path).map_err(|e| load_error(&path, &e))?;
    if let Some(mood) = mood {
        bundle.project.music = Some(kn_model::Music::new(kn_model::MusicSource::Mood(mood), 0.7));
    }
    let settings = ExportSettings::full_hd(bundle.project.aspect);
    let cancel = AtomicBool::new(false);
    let started = std::time::Instant::now();
    // Measured from the first frame, so start-up time doesn't skew the rate.
    let mut first: Option<std::time::Instant> = None;
    let mut frames = 0;
    kn_export::export_mp4(&bundle.project, &bundle.media, &settings, None, &out, &cancel, |done, total| {
        first.get_or_insert_with(std::time::Instant::now);
        frames = done;
        eprint!("\rframe {done}/{total}");
        let _ = std::io::stderr().flush();
    })
    .map_err(|e| format!("\ncouldn't make {}: {}", out.display(), match e.io() {
        Some(io) => io_hint(io),
        None => e.to_string(),
    }))?;
    let rate = first.map_or(0.0, |f| frames.saturating_sub(1) as f64 / f.elapsed().as_secs_f64().max(1e-9));
    eprintln!("\nwrote {} in {:.1} s ({rate:.0} frames/s)", out.display(), started.elapsed().as_secs_f64());
    Ok(())
}

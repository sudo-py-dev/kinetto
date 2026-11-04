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
  kinetto-cli export <project.kinetto> <video.mp4>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["sample", out] => sample(out.into()),
        ["info", project] => info(project.into()),
        ["export", project, out] => export(project.into(), out.into()),
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

/// Writes the starter deck, handy for trying export.
fn sample(out: PathBuf) -> Result<(), String> {
    let project = kn_model::Project::sample();
    storage::save(&project, &kn_media::MediaLibrary::new(), &out).map_err(|e| e.to_string())
}

fn info(path: PathBuf) -> Result<(), String> {
    let bundle = storage::load(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let p = &bundle.project;
    println!("slides:   {}", p.slides.len());
    println!("length:   {:.1} s", p.total_duration().as_secs_f64());
    println!("shape:    {:?}", p.aspect);
    println!("pictures: {}", bundle.media.ids().count());
    Ok(())
}

fn export(path: PathBuf, out: PathBuf) -> Result<(), String> {
    let bundle = storage::load(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let settings = ExportSettings::full_hd(bundle.project.aspect);
    let cancel = AtomicBool::new(false);
    let started = std::time::Instant::now();
    // Measured from the first frame, so start-up time doesn't skew the rate.
    let mut first: Option<std::time::Instant> = None;
    let mut frames = 0;
    kn_export::export_mp4(&bundle.project, &bundle.media, &settings, &out, &cancel, |done, total| {
        first.get_or_insert_with(std::time::Instant::now);
        frames = done;
        eprint!("\rframe {done}/{total}");
        let _ = std::io::stderr().flush();
    })
    .map_err(|e| format!("\n{e}"))?;
    let rate = first.map_or(0.0, |f| frames.saturating_sub(1) as f64 / f.elapsed().as_secs_f64().max(1e-9));
    eprintln!("\nwrote {} in {:.1} s ({rate:.0} frames/s)", out.display(), started.elapsed().as_secs_f64());
    Ok(())
}

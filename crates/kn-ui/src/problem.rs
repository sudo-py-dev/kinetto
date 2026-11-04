//! Turning failures into plain words: what happened, why (when it helps)
//! and what to do next. Failures of something the user just asked for are
//! shown in a dialog; ongoing ones in a banner; small notes in a toast.

use crate::export::Output;
use kn_engine::storage::StorageError;
use kn_export::{ExportError, GpuError};
use kn_model::LoadError;
use std::io;
use std::path::Path;

/// Something that went wrong, ready to show.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Problem {
    /// The dialog's heading, e.g. "Couldn't save “Talk.kinetto”".
    pub title: String,
    /// What happened and what to do, in plain words.
    pub body: String,
    /// A button that helps, shown next to Close.
    pub fix: Option<Fix>,
}

/// A button offered next to Close that leads out of the problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fix {
    /// Save to another place.
    SaveAs,
    /// Pick another file name or folder and make it again.
    ExportAgain(Output),
    /// Choose another file to import.
    Import,
    /// Choose another project to open.
    Open,
}

impl Fix {
    /// The fix button's text.
    pub fn label(self) -> &'static str {
        match self {
            Fix::SaveAs => "Save as…",
            Fix::ExportAgain(_) => "Try again…",
            Fix::Import => "Import another…",
            Fix::Open => "Open another…",
        }
    }
}

/// “file.ext”, for messages.
fn quoted(path: &Path) -> String {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    format!("\u{201c}{name}\u{201d}")
}

/// Why a file operation failed, and what usually fixes it.
pub(crate) fn io_reason(e: &io::Error) -> String {
    use io::ErrorKind as K;
    match e.kind() {
        K::NotFound => "It isn't there any more; it may have been moved, renamed or deleted.".into(),
        K::PermissionDenied => "Kinetto isn't allowed to use that place. Choose a folder of your own, such as Documents or Videos.".into(),
        K::StorageFull | K::QuotaExceeded => "The disk is full. Free up some space, or choose another drive.".into(),
        K::ReadOnlyFilesystem => "That place can only be read, not written (for example a locked USB stick). Choose another folder.".into(),
        K::FileTooLarge => "The file is too large for that drive. Choose another drive.".into(),
        K::IsADirectory => "That's a folder, not a file.".into(),
        K::TimedOut | K::Interrupted | K::ConnectionReset | K::NotConnected => {
            "The connection to that drive was lost. Check it's still connected, then try again.".into()
        }
        _ => format!("The system reported: {e}."),
    }
}

impl Problem {
    /// A problem with no fix button.
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Problem { title: title.into(), body: body.into(), fix: None }
    }

    /// Adds a fix button.
    pub fn with_fix(self, fix: Fix) -> Self {
        Problem { fix: Some(fix), ..self }
    }

    /// A file couldn't be read.
    pub fn read(path: &Path, e: &io::Error) -> Self {
        Problem::new(format!("Couldn't read {}", quoted(path)), io_reason(e))
    }

    /// A project couldn't be opened: missing, damaged, too new or unreadable.
    pub fn open(path: &Path, e: &StorageError) -> Self {
        let title = format!("Couldn't open {}", quoted(path));
        let body = match e {
            StorageError::Io(io) => io_reason(io),
            StorageError::Load(LoadError::TooNew(_)) => "It was made with a newer version of Kinetto. Update Kinetto to open it.".into(),
            StorageError::Load(LoadError::WrongFormat) => {
                "It isn't a Kinetto project. To make slides from a PowerPoint or Markdown file, use Import instead.".into()
            }
            StorageError::Load(LoadError::Json(_)) | StorageError::Zip(_) | StorageError::Damaged(_) => {
                "The file is damaged or isn't a Kinetto project. If you have another copy, try that one.".into()
            }
            StorageError::Encode(_) => "Something went wrong inside Kinetto. Please try again.".into(),
        };
        let fix = if matches!(e, StorageError::Load(LoadError::WrongFormat)) { Fix::Import } else { Fix::Open };
        Problem::new(title, body).with_fix(fix)
    }

    /// A project couldn't be saved; offers Save As.
    pub fn save(path: &Path, e: &StorageError) -> Self {
        let reason = match e {
            StorageError::Io(io) => io_reason(io),
            other => format!("Something went wrong inside Kinetto ({other})."),
        };
        Problem::new(format!("Couldn't save {}", quoted(path)), format!("{reason} Your work is still open here; nothing has been lost."))
            .with_fix(Fix::SaveAs)
    }

    /// `None` when the export was simply cancelled.
    pub fn export(output: Output, path: &Path, e: &ExportError) -> Option<Self> {
        let title = format!("Couldn't make {}", quoted(path));
        let body = match e {
            ExportError::Cancelled => return None,
            e if e.io().is_some() => io_reason(e.io().expect("checked")),
            ExportError::Gpu(GpuError::NoDevice) => {
                "Kinetto couldn't use the graphics card. Close other programs that use it heavily (games, video editors), update the graphics drivers, then try again.".into()
            }
            ExportError::Gpu(_) => "The graphics card stopped responding while drawing the video. Try again; restarting the computer often helps if it keeps happening.".into(),
            ExportError::Encoder(_) => "The video encoder stopped with an error. Try again; if it keeps happening, try another video shape.".into(),
            ExportError::Audio(_) => "The sound couldn't be encoded. Try changing the music, or remove the voice you added last, then try again.".into(),
            other => format!("Something went wrong inside Kinetto ({other}). Please try again."),
        };
        let note = " Your slides haven't changed.";
        Some(Problem::new(title, format!("{body}{note}")).with_fix(Fix::ExportAgain(output)))
    }

    /// A file couldn't be imported; offers to pick another.
    pub fn import(path: &Path, e: &kn_import::ImportError) -> Self {
        let body = match e {
            kn_import::ImportError::Damaged(_) => {
                "Kinetto couldn't read the slides in it. Open it in PowerPoint or LibreOffice, save it again as .pptx, then import that copy."
            }
            kn_import::ImportError::Empty => "There's no text or pictures in it to make slides from.",
        };
        Problem::new(format!("Couldn't import {}", quoted(path)), body).with_fix(Fix::Import)
    }

    /// A picture couldn't be used (unknown type, damaged, too large).
    pub fn picture(name: &str, e: &kn_media::MediaError) -> Self {
        use kn_media::MediaError as M;
        let body = match e {
            M::Unsupported => "Kinetto can use PNG, JPEG, WebP and GIF pictures. Save the picture in one of those formats, then add it again.",
            M::TooBig => "Pictures can be up to 100 MB and 16,384 pixels across. Make it smaller in a photo editor, then add it again.",
            M::Corrupt(_) => "The picture is damaged. Open it in a photo viewer; if it shows there, save a new copy as PNG or JPEG and add that.",
            M::Missing(_) => "It's missing from the project.",
        };
        Problem::new(format!("Couldn't add {name}"), body)
    }

    /// A sound couldn't be used.
    pub fn sound(name: &str, e: &kn_audio::AudioError) -> Self {
        let body = match e {
            kn_audio::AudioError::Unsupported => "Kinetto can use WAV, MP3, FLAC, OGG and M4A sounds. Convert it to one of those, then add it again.",
            kn_audio::AudioError::TooLong => "Sounds can be up to 30 minutes long. Trim it in an audio editor, then add it again.",
            kn_audio::AudioError::Resample(_) => "The sound couldn't be converted. Try saving it again as WAV or MP3.",
        };
        Problem::new(format!("Couldn't add {name}"), body)
    }

    /// A file is over the `limit_mb` size limit.
    pub fn too_big(path: &Path, limit_mb: u64) -> Self {
        Problem::new(format!("Couldn't add {}", quoted(path)), format!("Files can be up to {limit_mb} MB. Make it smaller first, then add it again."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_full_says_what_to_do() {
        let p = Problem::save(Path::new("/x/Talk.kinetto"), &StorageError::Io(io::Error::from(io::ErrorKind::StorageFull)));
        assert_eq!(p.title, "Couldn't save \u{201c}Talk.kinetto\u{201d}");
        assert!(p.body.contains("disk is full") && p.body.contains("nothing has been lost"));
        assert_eq!(p.fix, Some(Fix::SaveAs));
    }

    #[test]
    fn opening_a_non_project_suggests_import() {
        let p = Problem::open(Path::new("deck.kinetto"), &StorageError::Load(LoadError::WrongFormat));
        assert_eq!(p.fix, Some(Fix::Import));
        let newer = Problem::open(Path::new("a.kinetto"), &StorageError::Load(LoadError::TooNew(9)));
        assert!(newer.body.contains("Update Kinetto"));
    }

    #[test]
    fn cancelling_an_export_is_not_a_problem() {
        assert!(Problem::export(Output::Video, Path::new("a.mp4"), &ExportError::Cancelled).is_none());
        let no_gpu = Problem::export(Output::Video, Path::new("a.mp4"), &ExportError::Gpu(GpuError::NoDevice)).unwrap();
        assert!(no_gpu.body.contains("graphics drivers") && no_gpu.body.contains("haven't changed"));
    }

    #[test]
    fn every_io_kind_gets_words() {
        for kind in [io::ErrorKind::NotFound, io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            assert!(io_reason(&io::Error::from(kind)).ends_with('.'));
        }
    }
}

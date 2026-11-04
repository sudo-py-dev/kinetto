//! Saving that can't lose work: atomic writes and a crash-recovery copy.

use kn_model::{LoadError, Project};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "kinetto";

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("could not read or write the file: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error("could not encode the project: {0}")]
    Encode(#[from] serde_json::Error),
}

pub fn load(path: &Path) -> Result<Project, StorageError> {
    Ok(kn_model::from_json(&fs::read_to_string(path)?)?)
}

/// Writes to a temporary file beside `path`, syncs it, then renames it over
/// `path`. A crash at any point leaves either the old file or the new one.
pub fn save(project: &Project, path: &Path) -> Result<(), StorageError> {
    let json = kn_model::to_json(project)?;
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));

    let result = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        sync_dir(dir)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(result?)
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// The crash-recovery copy, written shortly after every edit and removed when
/// the work is saved or deliberately discarded.
pub struct Recovery {
    path: PathBuf,
}

impl Recovery {
    /// Uses the platform data folder, e.g. `~/.local/share/kinetto/Recovery`.
    pub fn in_data_dir() -> Option<Self> {
        let dir = dirs::data_dir()?.join("kinetto").join("Recovery");
        Some(Recovery::at(dir))
    }

    pub fn at(dir: PathBuf) -> Self {
        Recovery { path: dir.join(format!("unsaved.{EXTENSION}")) }
    }

    /// Unsaved work left by a previous run, if any.
    pub fn pending(&self) -> Option<Project> {
        load(&self.path).ok()
    }

    pub fn write(&self, project: &Project) -> Result<(), StorageError> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        save(project, &self.path)
    }

    pub fn clear(&self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kinetto-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_then_load_round_trips_and_leaves_no_temp_files() {
        let dir = temp_dir("save");
        let path = dir.join("talk.kinetto");
        let project = Project::sample();
        save(&project, &path).unwrap();
        save(&project, &path).unwrap();
        assert_eq!(load(&path).unwrap(), project);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recovery_round_trip() {
        let dir = temp_dir("recovery");
        let recovery = Recovery::at(dir.join("Recovery"));
        assert!(recovery.pending().is_none());
        recovery.write(&Project::sample()).unwrap();
        assert_eq!(recovery.pending(), Some(Project::sample()));
        recovery.clear();
        assert!(recovery.pending().is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}

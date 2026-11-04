//! Saving that can't lose work: atomic writes and a crash-recovery copy.
//!
//! A `.kinetto` file is a zip holding `project.json` and the pictures it
//! uses under `media/`. Files from before pictures existed are plain JSON
//! and still open.

use kn_media::{MAX_FILE_BYTES, MediaLibrary};
use kn_model::{LoadError, Project};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const EXTENSION: &str = "kinetto";
const PROJECT_ENTRY: &str = "project.json";
const MEDIA_DIR: &str = "media/";
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("could not read or write the file: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error("could not encode the project: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("the file is damaged: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("the file is damaged: {0}")]
    Damaged(&'static str),
}

/// A project and the pictures it uses.
pub struct Bundle {
    pub project: Project,
    pub media: MediaLibrary,
}

pub fn load(path: &Path) -> Result<Bundle, StorageError> {
    let bytes = fs::read(path)?;
    if !bytes.starts_with(ZIP_MAGIC) {
        let json = std::str::from_utf8(&bytes).map_err(|_| StorageError::Damaged("not a Kinetto project"))?;
        return Ok(Bundle { project: kn_model::from_json(json)?, media: MediaLibrary::new() });
    }

    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    let json = String::from_utf8(read_entry(&mut zip, PROJECT_ENTRY)?).map_err(|_| StorageError::Damaged("bad project text"))?;
    let project = kn_model::from_json(&json)?;
    let mut media = MediaLibrary::new();
    let names: Vec<String> = zip.file_names().filter_map(Result::ok).map(Into::into).collect();
    for name in names {
        if let Some(id) = name.strip_prefix(MEDIA_DIR).filter(|id| valid_media_id(id)) {
            let data = read_entry(&mut zip, &name)?;
            media.insert(id.to_owned(), data);
        }
    }
    Ok(Bundle { project, media })
}

/// Reads one entry, refusing anything bigger than a picture may be, so a
/// crafted file can't make us allocate without limit.
fn read_entry(zip: &mut ZipArchive<Cursor<Vec<u8>>>, name: &str) -> Result<Vec<u8>, StorageError> {
    let entry = zip.by_name(name)?;
    let mut out = Vec::new();
    entry.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut out)?;
    if out.len() > MAX_FILE_BYTES {
        return Err(StorageError::Damaged("an item is too large"));
    }
    Ok(out)
}

/// Media ids are a 32-digit hex hash plus a known extension.
fn valid_media_id(id: &str) -> bool {
    let Some((hash, ext)) = id.split_once('.') else { return false };
    let known = matches!(ext, "png" | "jpg" | "webp" | "gif") || kn_media::AUDIO_EXTENSIONS.contains(&ext);
    hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit()) && known
}

/// Saves `project` with only the pictures it still uses.
pub fn save(project: &Project, media: &MediaLibrary, path: &Path) -> Result<(), StorageError> {
    let json = kn_model::to_json(project)?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(PROJECT_ENTRY, SimpleFileOptions::default().compression_method(CompressionMethod::Deflated))?;
    zip.write_all(json.as_bytes())?;
    // Pictures are already compressed; store them as they are.
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for id in project.media_ids().collect::<BTreeSet<_>>() {
        if let Some(bytes) = media.get(id) {
            zip.start_file(format!("{MEDIA_DIR}{id}"), stored)?;
            zip.write_all(bytes)?;
        }
    }
    let bytes = zip.finish()?.into_inner();
    write_atomic(path, &bytes)?;
    Ok(())
}

/// Writes to a temporary file beside `path`, syncs it, then renames it over
/// `path`. A crash at any point leaves either the old file or the new one.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));

    let result = (|| {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        sync_dir(dir)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
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
    /// Uses the platform data folder, e.g. `~/.local/share/kinetto/Recovery`,
    /// or `$KINETTO_DATA_DIR/Recovery` when that is set (portable installs,
    /// tests).
    pub fn in_data_dir() -> Option<Self> {
        let base = match std::env::var_os("KINETTO_DATA_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => dirs::data_dir()?.join("kinetto"),
        };
        Some(Recovery::at(base.join("Recovery")))
    }

    pub fn at(dir: PathBuf) -> Self {
        Recovery { path: dir.join(format!("unsaved.{EXTENSION}")) }
    }

    /// Unsaved work left by a previous run, if any.
    pub fn pending(&self) -> Option<Bundle> {
        load(&self.path).ok()
    }

    pub fn write(&self, project: &Project, media: &MediaLibrary) -> Result<(), StorageError> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        save(project, media, &self.path)
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

    use kn_geom::Rect;
    use kn_model::Element;

    /// The sample project with one used and one unused picture.
    fn with_pictures() -> (Project, MediaLibrary, String, String) {
        let mut media = MediaLibrary::new();
        let used = media.import(kn_media::test_png(6, 6)).unwrap().id;
        let unused = media.import(kn_media::test_png(3, 9)).unwrap().id;
        let mut project = Project::sample();
        project.slides[0].elements.push(Element::image(used.clone(), Rect::new(0.0, 0.0, 100.0, 100.0)));
        (project, media, used, unused)
    }

    #[test]
    fn bundle_round_trips_keeps_only_used_media_and_leaves_no_temp_files() {
        let dir = temp_dir("save");
        let path = dir.join("talk.kinetto");
        let (project, media, used, unused) = with_pictures();
        save(&project, &media, &path).unwrap();
        save(&project, &media, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.project, project);
        assert_eq!(loaded.media.get(&used), media.get(&used));
        assert!(loaded.media.get(&unused).is_none());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn plain_json_files_from_phase_one_still_open() {
        let dir = temp_dir("legacy");
        let path = dir.join("old.kinetto");
        let v1 = kn_model::to_json(&Project::sample()).unwrap().replace(r#""schema_version":2"#, r#""schema_version":1"#);
        fs::write(&path, v1).unwrap();
        assert_eq!(load(&path).unwrap().project, Project::sample());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saved_archive_entries_use_current_timestamps() {
        let dir = temp_dir("timestamps");
        let path = dir.join("dated.kinetto");
        let (project, media, _, _) = with_pictures();
        let before = zip::DateTime::default_for_write();
        save(&project, &media, &path).unwrap();
        let after = zip::DateTime::default_for_write();
        let mut archive = ZipArchive::new(Cursor::new(fs::read(path).unwrap())).unwrap();
        for i in 0..archive.len() {
            let entry = archive.by_index(i).unwrap();
            let stamp = entry.last_modified().unwrap();
            assert!(stamp >= before && stamp <= after, "{:?} has stale date {stamp:?}", entry.name());
            assert_ne!(stamp.year(), 1980);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_damaged_files() {
        let dir = temp_dir("damaged");
        let path = dir.join("bad.kinetto");
        fs::write(&path, b"PK\x03\x04 definitely not a zip").unwrap();
        assert!(load(&path).is_err());
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(load(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn media_ids_are_checked() {
        assert!(valid_media_id("0123456789abcdef0123456789abcdef.png"));
        assert!(!valid_media_id("../../etc/passwd"));
        assert!(!valid_media_id("0123456789abcdef0123456789abcdef.exe"));
    }

    #[test]
    fn recovery_round_trip() {
        let dir = temp_dir("recovery");
        let recovery = Recovery::at(dir.join("Recovery"));
        assert!(recovery.pending().is_none());
        let (project, media, used, _) = with_pictures();
        recovery.write(&project, &media).unwrap();
        let back = recovery.pending().unwrap();
        assert_eq!(back.project, project);
        assert!(back.media.get(&used).is_some());
        recovery.clear();
        assert!(recovery.pending().is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}

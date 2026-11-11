//! Preferences kept between runs, in a small JSON file in the config folder
//! (e.g. `~/.config/kinetto/settings.json`). A missing or unreadable file
//! simply means the defaults.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How the editor is laid out. Both edit the same project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Layout {
    /// Slides in a sidebar: the simplest way to work.
    #[default]
    Simple,
    /// A timeline under the slide showing slides, items and sound in time.
    Timeline,
}

/// Preferences saved in the user's config folder as `kinetto/settings.json`.
/// Unknown or broken fields fall back to defaults instead of failing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Simple or Timeline editor layout.
    pub layout: Layout,
    /// Recently opened or saved projects, newest first.
    #[serde(default, deserialize_with = "recent_paths")]
    pub recent_projects: Vec<PathBuf>,
    /// Timeline zoom, in points per second.
    pub timeline_zoom: Option<f32>,
    /// Choices in the "Make a video in seconds" dialog.
    pub quick: crate::quick::QuickChoices,
    /// Quality choices in the export dialog.
    pub export: crate::export::ExportChoices,
    /// The language the editor is shown in; the system's on first run.
    pub language: crate::i18n::Language,
}

/// How many recent projects are kept.
const RECENT_LIMIT: usize = 20;

// One malformed history entry must not discard the user's other preferences.
/// Reads the recent list, skipping entries that aren't absolute paths,
/// duplicates and anything past [`RECENT_LIMIT`].
fn recent_paths<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<PathBuf>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let mut paths = Vec::new();
    if let Some(entries) = value.as_array() {
        for entry in entries {
            if let Some(path) = entry.as_str().filter(|s| !s.is_empty()) {
                let path = PathBuf::from(path);
                if !paths.contains(&path) && path.is_absolute() { paths.push(path); }
                if paths.len() == RECENT_LIMIT { break; }
            }
        }
    }
    Ok(paths)
}

/// Where settings are stored; `None` when the system has no config folder.
#[cfg(not(test))]
fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("kinetto").join("settings.json"))
}

/// Tests get a throwaway file per test thread, so they never read the
/// user's preferences or add their scratch projects to Recent projects.
/// The folder is private and randomly named (no one else can plant a link
/// in it), and is deleted when the test's thread ends.
#[cfg(test)]
fn path() -> Option<PathBuf> {
    thread_local! {
        static DIR: tempfile::TempDir = {
            let mut builder = tempfile::Builder::new();
            builder.prefix("kinetto-test-settings-");
            #[cfg(unix)]
            builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
            builder.tempdir().expect("a private temporary folder for test settings")
        };
    }
    Some(DIR.with(|dir| dir.path().join("settings.json")))
}

impl Settings {
    /// The saved settings, or defaults when there are none or they can't be read.
    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Moves `project` to the top of the recent list. Returns whether the list
    /// changed (it doesn't if the project is already first).
    pub fn remember_project(&mut self, project: &std::path::Path) -> std::io::Result<bool> {
        let project = std::fs::canonicalize(project)?;
        if self.recent_projects.first() == Some(&project) { return Ok(false); }
        self.recent_projects.retain(|p| p != &project);
        self.recent_projects.insert(0, project);
        self.recent_projects.truncate(RECENT_LIMIT);
        Ok(true)
    }

    /// Saves to the config folder, reporting failures.
    pub fn try_save(&self) -> std::io::Result<()> {
        let path = path().ok_or_else(|| std::io::Error::other("No configuration folder is available"))?;
        self.save_to(&path)
    }

    /// Saves to `path`, making its folder if needed.
    fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = (|| {
            std::fs::write(&temporary, json)?;
            std::fs::rename(&temporary, path)
        })();
        if result.is_err() { let _ = std::fs::remove_file(temporary); }
        result
    }

    /// Routine preference failures are logged; history actions show feedback.
    pub fn save(&self) {
        if let Err(err) = self.try_save() { tracing::warn!("couldn't save preferences: {err}"); }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_ignores_invalid_entries_without_losing_preferences() {
        let s: Settings = serde_json::from_str(r#"{"layout":"Timeline","recent_projects":[null,42,"","relative.kinetto","/tmp/a.kinetto","/tmp/a.kinetto","/tmp/b.kinetto"]}"#).unwrap();
        assert_eq!(s.layout, Layout::Timeline);
        assert_eq!(s.recent_projects, vec![PathBuf::from("/tmp/a.kinetto"), PathBuf::from("/tmp/b.kinetto")]);
        let s: Settings = serde_json::from_str(r#"{"recent_projects": {"broken":true}}"#).unwrap();
        assert!(s.recent_projects.is_empty());
    }

    #[test]
    fn history_orders_deduplicates_limits_and_rejects_missing_files() {
        let dir = std::env::temp_dir().join(format!("kinetto-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut settings = Settings::default();
        for i in 0..25 {
            let file = dir.join(format!("Project {i}.kinetto"));
            std::fs::write(&file, b"project").unwrap();
            settings.remember_project(&file).unwrap();
        }
        assert_eq!(settings.recent_projects.len(), 20);
        let file = dir.join("Project 6.kinetto");
        assert!(settings.remember_project(&file).unwrap());
        assert_eq!(settings.recent_projects[0], file.canonicalize().unwrap());
        assert!(!settings.remember_project(&file).unwrap());
        let before = settings.clone();
        assert!(settings.remember_project(&dir.join("missing.kinetto")).is_err());
        assert_eq!(settings, before);
        let config = dir.join("config/settings.json");
        settings.save_to(&config).unwrap();
        let back: Settings = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
        assert_eq!(back, settings);
        assert!(settings.save_to(&dir).is_err());
        assert!(file.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tests_never_use_the_users_settings_file() {
        let path = path().unwrap();
        assert!(path.starts_with(std::env::temp_dir()), "{}", path.display());
        if let Some(real) = dirs::config_dir() {
            assert!(!path.starts_with(real.join("kinetto")));
        }
        // Each test thread has its own file, so parallel tests don't collide.
        let other = std::thread::spawn(super::path).join().unwrap().unwrap();
        assert_ne!(path, other);
        // Only this user may enter the folder, and it goes when its thread ends.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "folder is open to others: {mode:o}");
        }
        assert!(!other.parent().unwrap().exists());
    }

    #[test]
    fn unknown_or_partial_files_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"layout":"Timeline"}"#).unwrap();
        assert_eq!(s.layout, Layout::Timeline);
        assert_eq!(s.timeline_zoom, None);
        let s: Settings = serde_json::from_str(r#"{"something_new":1}"#).unwrap();
        assert_eq!(s, Settings::default());
    }
}

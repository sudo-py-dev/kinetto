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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub layout: Layout,
    /// Timeline zoom, in points per second.
    pub timeline_zoom: Option<f32>,
    /// Choices in the "Make a video in seconds" dialog.
    pub quick: crate::quick::QuickChoices,
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("kinetto").join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Saving preferences is best-effort: a failure is logged, never shown.
    pub fn save(&self) {
        let Some(path) = path() else { return };
        let result = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
            let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
            std::fs::write(&path, json)
        });
        if let Err(err) = result {
            tracing::warn!("couldn't save preferences to {}: {err}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_partial_files_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"layout":"Timeline"}"#).unwrap();
        assert_eq!(s.layout, Layout::Timeline);
        assert_eq!(s.timeline_zoom, None);
        let s: Settings = serde_json::from_str(r#"{"something_new":1}"#).unwrap();
        assert_eq!(s, Settings::default());
    }
}

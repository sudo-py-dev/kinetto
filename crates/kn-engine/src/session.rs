//! An open project: the current state, undo history and save status.

use crate::command::{Command, CommandError};
use crate::storage::Bundle;
use kn_media::MediaLibrary;
use kn_model::Project;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Oldest undo steps are dropped past this.
const UNDO_LIMIT: usize = 200;
/// Edits further apart than this never merge into one undo step.
const COALESCE_WINDOW: Duration = Duration::from_millis(1000);

struct Step {
    before: Arc<Project>,
    label: &'static str,
    key: Option<(&'static str, usize, usize)>,
    at: Instant,
}

pub struct Session {
    project: Arc<Project>,
    /// Pictures added so far. Kept outside undo: undoing an added picture
    /// removes its element, and unused pictures are left out when saving.
    pub media: MediaLibrary,
    undo: Vec<Step>,
    redo: Vec<(Arc<Project>, &'static str)>,
    /// Set by [`Session::seal`] when a gesture ends, so the next edit starts
    /// a new undo step even if it has the same key.
    sealed: bool,
    pub path: Option<PathBuf>,
    /// Bumped on every change; lets views cache work per revision.
    revision: u64,
    saved_revision: u64,
    last_edit: Option<Instant>,
}

impl Session {
    pub fn new(project: Project) -> Self {
        Session {
            project: Arc::new(project),
            media: MediaLibrary::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            sealed: true,
            path: None,
            revision: 0,
            saved_revision: 0,
            last_edit: None,
        }
    }

    /// A session for a loaded bundle; `path` is `None` for recovered work.
    pub fn from_bundle(bundle: Bundle, path: Option<PathBuf>) -> Self {
        Session { media: bundle.media, path, ..Session::new(bundle.project) }
    }

    pub fn project(&self) -> &Arc<Project> {
        &self.project
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn mark_saved(&mut self, revision: u64) {
        self.saved_revision = revision;
    }

    /// Time since the most recent edit, if any edit is unsaved.
    pub fn idle_since_edit(&self) -> Option<Duration> {
        self.is_dirty().then(|| self.last_edit.map_or(Duration::MAX, |t| t.elapsed()))
    }

    pub fn execute(&mut self, command: Command) -> Result<(), CommandError> {
        let next = Arc::new(command.apply(&self.project)?);
        let key = command.coalesce_key();
        let now = Instant::now();
        let merge = !self.sealed
            && key.is_some()
            && self.undo.last().is_some_and(|s| s.key == key && now.duration_since(s.at) < COALESCE_WINDOW);
        if merge {
            // Keep the original "before" so one undo reverts the whole burst.
            if let Some(step) = self.undo.last_mut() {
                step.at = now;
            }
        } else {
            self.undo.push(Step { before: Arc::clone(&self.project), label: command.label(), key, at: now });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.sealed = false;
        self.redo.clear();
        self.replace(next);
        Ok(())
    }

    /// Ends the current gesture (mouse released, text field left).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(|s| s.label)
    }

    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(|(_, label)| *label)
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else { return false };
        self.redo.push((Arc::clone(&self.project), step.label));
        self.sealed = true;
        self.replace(step.before);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some((after, label)) = self.redo.pop() else { return false };
        self.undo.push(Step { before: Arc::clone(&self.project), label, key: None, at: Instant::now() });
        self.sealed = true;
        self.replace(after);
        true
    }

    fn replace(&mut self, project: Arc<Project>) {
        self.project = project;
        self.revision += 1;
        self.last_edit = Some(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kn_geom::{Color, Rect};

    fn moved(x: f64) -> Command {
        Command::SetFrame { slide: 0, index: 0, frame: Rect::new(x, 0.0, 10.0, 10.0) }
    }

    #[test]
    fn every_command_undoes_exactly() {
        let commands = [
            Command::AddSlide { at: 1 },
            Command::DuplicateSlide { index: 0 },
            Command::DeleteSlide { index: 1 },
            Command::MoveSlide { from: 0, to: 1 },
            Command::SetBackground { index: 0, color: Color::rgb(1, 2, 3) },
            Command::SetDuration { index: 0, millis: 5000 },
            Command::SetTransition { index: 0, transition: kn_model::Transition::Zoom },
            Command::DeleteElement { slide: 0, index: 0 },
            moved(42.0),
            Command::ReorderElement { slide: 0, from: 0, to: 1 },
            Command::SetAspect { aspect: kn_geom::Aspect::Square1x1 },
            Command::ApplyTheme { theme: "Midnight".into() },
            Command::AddLayoutSlide { at: 1, layout: kn_layout::Layout::Quote },
            Command::SetDark { index: 0, dark: true },
            Command::SetEntrance { index: 0, entrance: kn_model::Entrance::Together },
            Command::SetMusic { music: Some(kn_model::Music::new(kn_model::MusicSource::Mood(kn_model::Mood::Epic), 0.5)) },
            Command::SetVoice { index: 0, voice: Some("0123456789abcdef0123456789abcdef.wav".into()), fit_millis: 9000 },
        ];
        for command in commands {
            let mut s = Session::new(Project::sample());
            let original = s.project().clone();
            s.execute(command.clone()).unwrap();
            assert_ne!(*s.project(), original, "{command:?} changed nothing");
            assert!(s.undo());
            assert_eq!(*s.project(), original, "{command:?} did not undo exactly");
            assert!(s.redo());
            assert_eq!(**s.project(), command.apply(&original).unwrap());
        }
    }

    #[test]
    fn drag_merges_into_one_step_until_sealed() {
        let mut s = Session::new(Project::sample());
        let original = s.project().clone();
        for x in 0..20 {
            s.execute(moved(f64::from(x))).unwrap();
        }
        s.seal();
        s.execute(moved(99.0)).unwrap();
        assert!(s.undo());
        assert_eq!(s.project().slides[0].elements[0].frame.x, 19.0);
        assert!(s.undo());
        assert_eq!(*s.project(), original);
        assert!(!s.undo());
    }

    #[test]
    fn failed_command_changes_nothing() {
        let mut s = Session::new(Project::sample());
        let r = s.revision();
        assert_eq!(s.execute(Command::DeleteSlide { index: 9 }), Err(CommandError::NoSlide(9)));
        assert_eq!(s.revision(), r);
        assert!(s.undo_label().is_none());
    }

    #[test]
    fn last_slide_cannot_be_deleted() {
        let mut s = Session::new(Project::sample());
        s.execute(Command::DeleteSlide { index: 0 }).unwrap();
        assert_eq!(s.execute(Command::DeleteSlide { index: 0 }), Err(CommandError::LastSlide));
    }

    #[test]
    fn slide_looks_are_independent_and_apply_all_undoes_atomically() {
        let original = Project::sample();
        let mut s = Session::new(original.clone());
        s.execute(Command::ApplySlideTheme { index: 0, theme: "Ocean".into() }).unwrap();
        assert_eq!(s.project().slides[1], original.slides[1]);
        assert_eq!(s.project().slides[0].theme.as_deref(), Some("Ocean"));
        s.execute(Command::SetDark { index: 0, dark: true }).unwrap();
        assert_eq!(s.project().slides[1], original.slides[1]);
        let mixed = s.project().clone();
        let restored: Project = serde_json::from_str(&serde_json::to_string(&*mixed).unwrap()).unwrap();
        assert_eq!(restored, *mixed);
        s.execute(Command::ApplySlideLookToAll { index: 0 }).unwrap();
        assert_eq!(s.project().theme, "Ocean");
        for slide in &s.project().slides {
            assert!(slide.dark);
            assert_eq!(slide.background, kn_layout::theme("Ocean").dark_background);
            assert_eq!(slide.theme_name(s.project()), "Ocean");
        }
        assert!(s.undo());
        assert_eq!(s.project(), &mixed);
        assert!(s.redo());
        assert!(s.project().slides.iter().all(|slide| slide.dark));
    }

    #[test]
    fn music_uses_absolute_video_time_when_slides_move_and_undo_restores_it() {
        let mut project = Project::sample();
        let mut music = kn_model::Music::new(kn_model::MusicSource::Mood(kn_model::Mood::Calm), 0.4);
        music.move_to(1789, project.total_duration());
        project.music = Some(music.clone());
        let mut session = Session::new(project);
        session.execute(Command::MoveSlide { from: 0, to: 1 }).unwrap();
        assert_eq!(session.project().music.as_ref(), Some(&music));
        session.execute(Command::SetDuration { index: 0, millis: 5000 }).unwrap();
        assert_eq!(session.project().music.as_ref(), Some(&music));
        let mut moved = music.clone();
        moved.move_to(3456, session.project().total_duration());
        session.execute(Command::SetMusic { music: Some(moved.clone()) }).unwrap();
        session.seal();
        assert_eq!(session.project().music.as_ref(), Some(&moved));
        assert!(session.undo());
        assert_eq!(session.project().music.as_ref(), Some(&music));
        assert!(session.redo());
        assert_eq!(session.project().music.as_ref(), Some(&moved));
        let saved = serde_json::to_string(&**session.project()).unwrap();
        let loaded: Project = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.music, Some(moved));
    }

    #[test]
    fn commands_have_stable_json_ids() {
        let json = serde_json::to_string(&Command::AddSlide { at: 2 }).unwrap();
        assert_eq!(json, r#"{"id":"slide.add","at":2}"#);
    }
}

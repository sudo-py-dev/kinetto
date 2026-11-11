//! Renders the real window to PNGs for a visual check. Runs only when
//! `KINETTO_SCREENS` names an output folder.

use super::{KinettoApp, Problem, problem, storage};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// Shows `session` in the editor, away from the user's real safety copy
/// and settings; `layout` picks Simple or Timeline when given.
fn open_editor(h: &mut Harness<'_, KinettoApp>, session: kn_engine::Session, layout: Option<super::settings::Layout>) {
    let app = h.state_mut();
    app.session = session;
    app.recovery = None;
    app.restorable = None;
    app.choosing_template = false;
    if let Some(layout) = layout {
        app.settings.layout = layout;
    }
}

#[test]
fn transport_finder_and_item_shortcuts_work_and_undo_in_both_themes() {
    let screenshots = std::env::var_os("KINETTO_POWER_SCREENS").map(std::path::PathBuf::from);
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let builder = Harness::builder().with_size([1280.0, 800.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        let project = kn_layout::template(kn_layout::Template::ALL[0]);
        let media = kn_media::sample_library();
        open_editor(&mut h, kn_engine::Session::from_bundle(storage::Bundle { project, media }, None), Some(super::settings::Layout::Simple));
        h.run_steps(4);
        let shot = |h: &mut Harness<'_, KinettoApp>, what: &str| {
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("{what}-{name}.png"))).unwrap(); }
        };

        // The play controls under the slide move between slides.
        assert!(h.query_by_label("Video position").is_some());
        h.get_by_label("Next slide  (Page Down)").click();
        h.run_steps(4);
        assert_eq!(h.state().slide, 1);
        h.key_press(egui::Key::PageUp);
        h.run_steps(4);
        assert_eq!(h.state().slide, 0);
        shot(&mut h, "power-editor");

        // Ctrl+D copies the selected item as one undo step.
        let count = h.state().project().slides[0].elements.len();
        h.state_mut().selected = Some(count - 1);
        h.run_steps(4);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
        h.run_steps(4);
        assert_eq!(h.state().project().slides[0].elements.len(), count + 1);
        assert_eq!(h.state().selected, Some(count));
        shot(&mut h, "power-item");
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project().slides[0].elements.len(), count);

        // Copy on one slide, paste on another.
        h.state_mut().selected = Some(0);
        let ctx = h.ctx.clone();
        h.state_mut().copy_selected(&ctx);
        h.state_mut().select_slide(1);
        let before = h.state().project().slides[1].elements.len();
        h.state_mut().paste_from_menu(&ctx);
        h.run_steps(4);
        assert_eq!(h.state().project().slides[1].elements.len(), before + 1);
        assert!(h.state_mut().session.undo());
        h.state_mut().select_slide(0);
        h.run_steps(4);

        // Right-click offers the item's menu (the slide needs a GPU to be drawn).
        h.state_mut().selected = None;
        h.run_steps(2);
        if screenshots.is_some() {
        let centre = egui::pos2(1280.0 / 2.0 - 60.0, 400.0);
        h.hover_at(centre);
        h.event(egui::Event::PointerButton { pos: centre, button: egui::PointerButton::Secondary, pressed: true, modifiers: egui::Modifiers::NONE });
        h.event(egui::Event::PointerButton { pos: centre, button: egui::PointerButton::Secondary, pressed: false, modifiers: egui::Modifiers::NONE });
        h.run_steps(4);
        assert!(h.query_by_label("Paste").is_some());
        shot(&mut h, "power-menu");
        h.key_press(egui::Key::Escape);
        h.run_steps(4);
        }

        // The finder narrows as you type and Enter runs the top result.
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
        h.run_steps(4);
        assert!(h.state().finder.is_some());
        shot(&mut h, "power-finder");
        h.get_by_label("Find an action").type_text("shape square");
        h.run_steps(4);
        shot(&mut h, "power-finder-typed");
        h.key_press(egui::Key::Enter);
        h.run_steps(4);
        assert!(h.state().finder.is_none());
        assert_eq!(h.state().project().aspect, kn_geom::Aspect::Square1x1);
        assert!(h.state_mut().session.undo());

        // Slide settings come as three short pages.
        for (tab, page) in [("Sound", "sound"), ("Video", "video"), ("Slide", "slide")] {
            h.get_by_role_and_label(egui::Role::RadioButton, tab).click();
            h.run_steps(4);
            shot(&mut h, &format!("power-settings-{page}"));
        }
        assert_eq!(h.state().inspector_tab, super::inspector::Tab::Slide);
        assert!(h.query_by_label("Timing").is_some());

        // F1 lists the shortcuts; Escape closes the list.
        h.key_press(egui::Key::F1);
        h.run_steps(4);
        assert!(h.state().shortcuts_open);
        shot(&mut h, "power-shortcuts");
        h.key_press(egui::Key::Escape);
        h.run_steps(4);
        assert!(!h.state().shortcuts_open);
    }
}

#[test]
fn editor_navigation_and_alignment_are_clear_in_both_themes() {
    let screenshots = std::env::var_os("KINETTO_LAYOUT_SCREENS").map(std::path::PathBuf::from);
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        for width in [960.0, 1440.0] {
            let builder = Harness::builder().with_size([width, 1000.0]).with_theme(theme);
            let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
            let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
            open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
            h.run_steps(3);
            let slides = h.get_by_role_and_label(egui::Role::RadioButton, "Slides").rect();
            let timeline = h.get_by_role_and_label(egui::Role::RadioButton, "Timeline").rect();
            let present = h.get_by_label("Present").rect();
            assert!(slides.right() <= timeline.left());
            assert!(timeline.right() < present.left());
            h.get_by_role_and_label(egui::Role::RadioButton, "Timeline").click();
            h.run_steps(3);
            assert_eq!(h.state().settings.layout, super::settings::Layout::Timeline);
            assert!(h.get_by_label("Fit").rect().right() <= width - 328.0);
            if let Some(dir) = &screenshots {
                h.render().unwrap().save(dir.join(format!("editor-{theme:?}-{width:.0}.png"))).unwrap();
            }
            h.state_mut().selected = Some(0);
            h.run_steps(3);
            h.get_by_label("Align on slide").click();
            h.run_steps(3);
            let before = h.state().project().slides[0].elements[0].frame;
            h.get_by_role_and_label(egui::Role::Button, "Right").click();
            h.run_steps(3);
            let (cw, _) = h.state().project().aspect.canvas_size();
            assert_eq!(h.state().project().slides[0].elements[0].frame.x, cw - before.w);
            h.state_mut().session.undo();
            h.run_steps(3);
            assert_eq!(h.state().project().slides[0].elements[0].frame, before);
            h.get_by_label("Slide settings").click();
            h.run_steps(3);
            assert!(h.state().selected.is_none());
            h.get_by_role_and_label(egui::Role::RadioButton, "Slides").click();
            h.run_steps(3);
            assert_eq!(h.state().settings.layout, super::settings::Layout::Simple);
        }
    }
}

#[test]
fn soundtrack_selection_and_placement_follow_slide_and_scrubbed_playhead() {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut h = Harness::builder().with_size([1280.0, 1100.0]).with_theme(theme).build_eframe(|cc| KinettoApp::new(cc));
        open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
        h.state_mut().slide = 1;
        h.state_mut().selected = None;
        h.state_mut().inspector_tab = super::inspector::Tab::Sound;
        h.state_mut().choose_soundtrack(Some(kn_model::MusicSource::Mood(kn_model::Mood::Calm)));
        assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 3000);
        assert_eq!(h.state().project().music.as_ref().unwrap().offset_ms, 0);
        h.state_mut().scrub = Some(kn_time::Tick::from_millis(4321));
        h.run_steps(4);
        let buttons = ["Start at this slide", "Start at playhead", "From video start"]
            .map(|label| h.get_by_label(label).rect());
        for pair in buttons.windows(2) {
            assert!((pair[0].left() - pair[1].left()).abs() < 1.0);
            assert!((pair[0].width() - pair[1].width()).abs() < 1.0);
            assert!(pair[1].top() >= pair[0].bottom() + 5.0, "music buttons must have separate rows and spacing");
        }
        h.get_by_label("Start at playhead").scroll_to_me(); h.run_steps(4);
        h.get_by_label("Start at playhead").click(); h.run_steps(4);
        assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 4321);
        h.get_by_label("From video start").click(); h.run_steps(4);
        assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 0);
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 4321);
        h.state_mut().choose_soundtrack(Some(kn_model::MusicSource::Mood(kn_model::Mood::Calm)));
        assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 3000);
    }
}

#[test]
fn timeline_music_volume_and_removal_support_undo_and_show_audio_peaks() {
    let screenshots = std::env::var_os("KINETTO_AUDIO_SCREENS").map(std::path::PathBuf::from);
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let builder = Harness::builder().with_size([1440.0, 1000.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        let mut media = kn_media::MediaLibrary::new();
        let pcm = kn_audio::generate(kn_model::Mood::Upbeat, kn_audio::RATE as usize * 3);
        let id = media.import_audio(pcm.to_wav()).unwrap();
        let mut project = kn_model::Project::sample();
        project.slides[0].elements.truncate(1);
        project.music = Some(kn_model::Music::new(kn_model::MusicSource::File(id.clone()), 0.6));
        project.slides[0].voice = Some(id);
        h.state_mut().session = kn_engine::Session::from_bundle(storage::Bundle { project: project.clone(), media }, None);
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.state_mut().choosing_template = false;
        h.state_mut().settings.layout = super::settings::Layout::Timeline;
        h.state_mut().settings.timeline_zoom = Some(100.0);
        h.run_steps(4);
        assert_eq!(h.state().waveform.len(), 1, "Music and voice share the source peak cache");
        h.get_by_role_and_label(egui::Role::Slider, "Timeline music volume").set_value_accesskit(25.0);
        h.run_steps(4);
        assert_eq!(h.state().project().music.as_ref().unwrap().volume, 0.25);
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("audio-timeline-{name}.png"))).unwrap(); }
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project(), &project);
        h.run_steps(4);
        h.get_by_label("Remove music").click();
        h.run_steps(4);
        assert!(h.state().project().music.is_none());
        assert!(h.state().project().slides[0].voice.is_some());
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project(), &project);
    }
}

#[test]
fn full_length_music_drags_freely_and_undoes_in_one_step() {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut h = Harness::builder().with_size([1280.0, 900.0]).with_theme(theme).build_eframe(|cc| KinettoApp::new(cc));
        let mut project = kn_model::Project::sample();
        project.music = Some(kn_model::Music::new(kn_model::MusicSource::Mood(kn_model::Mood::Calm), 0.4));
        open_editor(&mut h, kn_engine::Session::new(project.clone()), Some(super::settings::Layout::Timeline));
        h.state_mut().settings.timeline_zoom = Some(60.0);
        h.run_steps(4);
        let origin = h.get_by_label("Move music clip").rect().center();
        h.drag_at(origin); h.run_steps(2);
        let destination = origin + egui::vec2(140.7, 0.0);
        h.hover_at(destination); h.run_steps(4);
        h.drop_at(destination); h.run_steps(4);
        let music = h.state().project().music.as_ref().unwrap();
        assert!((2343..=2346).contains(&music.start_ms), "{music:?}");
        assert_eq!(music.end_ms, None);
        assert_eq!(music.offset_ms, 0);
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project(), &project);
        assert!(h.state_mut().session.redo());
        assert!((2343..=2346).contains(&h.state().project().music.as_ref().unwrap().start_ms));
    }
}

#[test]
fn trimmed_music_restores_length_when_dragging_back_and_shift_snaps() {
    let mut h = Harness::builder().with_size([1280.0, 900.0]).build_eframe(|cc| KinettoApp::new(cc));
    let mut project = kn_model::Project::sample();
    let mut music = kn_model::Music::new(kn_model::MusicSource::Mood(kn_model::Mood::Calm), 0.4);
    music.end_ms = Some(4000); music.offset_ms = 700;
    project.music = Some(music);
    open_editor(&mut h, kn_engine::Session::new(project.clone()), Some(super::settings::Layout::Timeline));
    h.state_mut().settings.timeline_zoom = Some(60.0);
    h.run_steps(4);
    let origin = h.get_by_label("Move music clip").rect().center();
    h.drag_at(origin); h.run_steps(2);
    h.hover_at(origin + egui::vec2(360.0, 0.0)); h.run_steps(4);
    let m = h.state().project().music.as_ref().unwrap();
    assert_eq!((m.start_ms, m.end_ms), (6000, Some(7000)));
    let back = origin + egui::vec2(75.0, 0.0);
    h.hover_at(back); h.run_steps(4);
    let m = h.state().project().music.as_ref().unwrap();
    assert_eq!((m.start_ms, m.end_ms, m.offset_ms), (1250, Some(5250), 700));
    h.drop_at(back); h.run_steps(4);
    assert!(h.state_mut().session.undo());
    assert_eq!(h.state().project(), &project);
    h.run_steps(4);
    let origin = h.get_by_label("Move music clip").rect().center();
    h.drag_at(origin); h.run_steps(2);
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    let near_boundary = origin + egui::vec2(176.7, 0.0);
    h.hover_at(near_boundary); h.run_steps(4);
    assert_eq!(h.state().project().music.as_ref().unwrap().start_ms, 3000);
    h.drop_at(near_boundary); h.run_steps(4);
    assert!(h.state_mut().session.undo());
    assert_eq!(h.state().project(), &project);
}

#[test]
fn ruler_click_and_outside_release_never_leave_scrubbing_attached() {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut h = Harness::builder().with_size([1280.0, 900.0]).with_theme(theme).build_eframe(|cc| KinettoApp::new(cc));
        open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Timeline));
        h.run_steps(4);
        let ruler = h.get_by_label("Timeline ruler").rect();
        let at = egui::pos2(ruler.left() + 100.0, ruler.center().y);
        h.drag_at(at); h.run_steps(2); h.drop_at(at); h.run_steps(4);
        assert!(h.state().timeline_drag.is_none());
        let selected = h.state().scrub;
        assert!(selected.is_some());
        h.hover_at(at + egui::vec2(100.0, 0.0)); h.run_steps(4);
        assert_eq!(h.state().scrub, selected);
        h.drag_at(at); h.run_steps(2);
        h.hover_at(at + egui::vec2(80.0, 0.0)); h.run_steps(4);
        assert!(matches!(h.state().timeline_drag, Some(super::timeline::TimelineDrag::Scrub)));
        h.drop_at(egui::pos2(20.0, 20.0)); h.run_steps(4);
        assert!(h.state().timeline_drag.is_none());
        let released = h.state().scrub;
        h.hover_at(at + egui::vec2(180.0, 0.0)); h.run_steps(4);
        assert_eq!(h.state().scrub, released);
        h.drag_at(at); h.run_steps(2);
        h.hover_at(at + egui::vec2(90.0, 0.0)); h.run_steps(4);
        h.event(egui::Event::WindowFocused(false)); h.run_steps(4);
        assert!(h.state().timeline_drag.is_none());
    }
}

#[test]
fn playback_pages_the_timeline_without_continuously_chasing_the_head() {
    let mut h = Harness::builder().with_size([1280.0, 900.0]).build_eframe(|cc| KinettoApp::new(cc));
    let mut project = kn_model::Project::sample();
    project.slides = vec![project.slides[0].clone(); 30];
    open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Timeline));
    h.state_mut().settings.timeline_zoom = Some(60.0);
    h.run_steps(4);
    let initial = h.get_by_label("Timeline ruler").rect().left();
    h.state_mut().playback.playing = true;
    h.state_mut().playback.audio_clock = false;
    h.state_mut().playback.origin = kn_time::Tick::from_secs(25);
    h.state_mut().playback.started = std::time::Instant::now();
    h.run_steps(8);
    let paged = h.get_by_label("Timeline ruler").rect().left();
    assert!(paged < initial - 500.0, "timeline did not page: {initial} -> {paged}");
    h.run_steps(8);
    let settled = h.get_by_label("Timeline ruler").rect().left();
    assert!((settled - paged).abs() < 2.0, "timeline kept chasing the head");
    h.state_mut().playback.origin = kn_time::Tick::ZERO;
    h.state_mut().playback.started = std::time::Instant::now();
    h.run_steps(8);
    let restarted = h.get_by_label("Timeline ruler").rect().left();
    assert!((restarted - initial).abs() < 2.0, "restart did not return to the first page");
    h.state_mut().stop_playback();
}

#[test]
fn desktop_project_open_loads_the_file_and_reports_missing_files() {
    let dir = std::env::temp_dir().join(format!("kinetto-desktop-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("My video with spaces.kinetto");
    let project = kn_layout::template(kn_layout::Template::Travel);
    storage::save(&project, &kn_media::sample_library(), &path).unwrap();
    let mut h = Harness::builder().build_eframe(|cc| KinettoApp::new(cc));
    h.state_mut().recovery = None;
    h.state_mut().open_project(path.clone());
    assert_eq!(h.state().project(), &project);
    assert_eq!(h.state().session.path.as_ref(), Some(&path));
    assert_eq!(h.state().settings.recent_projects.first(), Some(&path.canonicalize().unwrap()));
    assert!(!h.state().choosing_template);
    assert!(!h.state().session.is_dirty());
    h.state_mut().open_project(dir.join("missing.kinetto"));
    assert!(h.state().problem.is_some());
    assert_eq!(h.state().project(), &project);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recent_project_row_opens_the_editor_from_welcome() {
    let dir = std::env::temp_dir().join(format!("kinetto-recent-row-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("My recent video with a longer project name.kinetto");
    let project = kn_layout::template(kn_layout::Template::Travel);
    storage::save(&project, &kn_media::sample_library(), &path).unwrap();
    let path = path.canonicalize().unwrap();
    let screenshots = std::env::var_os("KINETTO_WELCOME_SCREENS").map(std::path::PathBuf::from);
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let builder = Harness::builder().with_size([1440.0, 960.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.state_mut().settings = super::settings::Settings::default();
        h.state_mut().settings.recent_projects = vec![path.clone()];
        h.run_steps(4);
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("welcome-recent-{name}.png"))).unwrap(); }
        h.set_size(egui::vec2(760.0, 850.0));
        h.run_steps(4);
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("welcome-narrow-{name}.png"))).unwrap(); }
        h.get_by_label("Open My recent video with a longer project name").click();
        h.run_steps(4);
        assert!(!h.state().choosing_template);
        assert!(h.state().pending.is_none());
        assert_eq!(h.state().project(), &project);
        assert_eq!(h.state().session.path.as_ref(), Some(&path));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn recent_project_failures_preserve_the_open_project_and_history() {
    let dir = std::env::temp_dir().join(format!("kinetto-recent-errors-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let broken = dir.join("broken.kinetto");
    std::fs::write(&broken, b"not a project").unwrap();
    let missing = dir.join("missing.kinetto");
    let mut h = Harness::builder().build_eframe(|cc| KinettoApp::new(cc));
    h.state_mut().recovery = None;
    h.state_mut().restorable = None;
    h.state_mut().settings.recent_projects = vec![missing.clone(), broken.clone()];
    let before = h.state().project().clone();
    for path in [missing, broken] {
        h.state_mut().perform(super::Pending::Open(path));
        assert!(h.state().problem.is_some());
        assert_eq!(h.state().project(), &before);
        assert_eq!(h.state().settings.recent_projects.len(), 2);
    }
    h.state_mut().problem = None;
    h.run_steps(4);
    assert!(h.query_by_label("File moved or missing").is_some());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sidebar_animation_changes_only_the_following_slide_and_can_be_undone() {
    let mut h = Harness::builder().with_size([1280.0, 800.0]).build_eframe(|cc| KinettoApp::new(cc));
    let mut project = kn_model::Project::sample();
    project.slides.push(project.slides[0].clone());
    open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Simple));
    h.state_mut().playback.player = None;
    h.state_mut().told_silent = true;
    h.state_mut().selected = Some(1);
    h.run_steps(4);

    h.get_by_label("Animation between slides 1 and 2").click();
    h.run_steps(4);
    h.get_by_label("3D left").scroll_to_me();
    h.run_steps(4);
    h.get_by_label("3D left").click();
    h.run_steps(4);
    assert_eq!(h.state().project().slides[1].transition, kn_model::Transition::TurnLeft);
    assert_eq!(h.state().project().slides[0].transition, kn_model::Transition::Cut);
    assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Cut);
    assert!(h.state().playback.playing);
    assert!(h.state().selected.is_none());
    h.state_mut().stop_playback();

    h.get_by_label("Animation between slides 2 and 3").click();
    h.run_steps(4);
    h.get_by_label("Wipe").scroll_to_me();
    h.run_steps(4);
    h.get_by_label("Wipe").click();
    h.run_steps(4);
    assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Wipe);
    assert_eq!(h.state().project().slides[1].transition, kn_model::Transition::TurnLeft);
    h.state_mut().stop_playback();
    assert!(h.state_mut().session.undo());
    assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Cut);
    assert_eq!(h.state().project().slides[1].transition, kn_model::Transition::TurnLeft);
    h.get_by_label("Animation between slides 2 and 3").click();
    h.run_steps(4);
    h.get_by_label("Spin").scroll_to_me();
    h.run_steps(4);
    h.get_by_label("Spin").click();
    h.run_steps(4);
    assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Spin);
    h.state_mut().stop_playback();
    assert!(h.state_mut().session.undo());
    assert_eq!(h.state().project().slides[2].transition, kn_model::Transition::Cut);
}

#[test]
fn closing_an_untitled_video_requires_a_decision_and_keeps_a_safety_copy() {
    let directory = std::env::temp_dir().join(format!("kinetto-close-untitled-{}",std::process::id()));
    let screenshots = std::env::var_os("KINETTO_CLOSE_SCREENS").map(std::path::PathBuf::from);
    let builder = Harness::builder().with_size([1100.0,800.0]);
    let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
    let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
    h.state_mut().recovery = None;
    h.state_mut().restorable = None;
    let bundle = storage::Bundle { project:kn_layout::template(kn_layout::Template::Promo),media:kn_media::sample_library() };
    h.state_mut().replace_session(kn_engine::Session::from_bundle(bundle,None));
    h.state_mut().recovery = Some(storage::Recovery::at(directory.clone()));
    h.state_mut().choosing_template = false;
    assert!(!h.state().session.is_dirty());
    h.run_steps(4);
    let recovery = storage::Recovery::at(directory.clone()).pending().unwrap();
    for id in recovery.project.media_ids() { assert!(recovery.media.get(id).is_some()); }
    h.get_by_label("Close").click();
    h.run_steps(4);
    assert!(matches!(h.state().pending,Some(super::Pending::Close)));
    assert!(!h.state().close_confirmed);
    if let Some(directory) = &screenshots {
        std::fs::create_dir_all(directory).unwrap();
        h.render().unwrap().save(directory.join("save-before-closing.png")).unwrap();
    }
    h.get_by_label("Cancel").click();
    h.run_steps(4);
    assert!(h.state().pending.is_none());
    assert!(!h.state().close_confirmed);
    h.input_mut().viewports.get_mut(&egui::ViewportId::ROOT).unwrap().events.push(egui::ViewportEvent::Close);
    h.run_steps(1);
    assert!(h.output().viewport_output[&egui::ViewportId::ROOT].commands.contains(&egui::ViewportCommand::CancelClose));
    h.input_mut().viewports.get_mut(&egui::ViewportId::ROOT).unwrap().events.clear();
    h.run_steps(4);
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(h.state().pending.is_none());
    assert!(!h.state().close_confirmed);
    h.get_by_label("Close").click();
    h.run_steps(4);
    h.get_by_label("Discard").click();
    h.run_steps(4);
    assert!(h.state().close_confirmed && h.state().discard_on_close);
    eframe::App::on_exit(h.state_mut());
    assert!(storage::Recovery::at(directory.clone()).pending().is_none());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn save_and_close_waits_for_success_and_discard_preserves_the_saved_file() {
    let directory = std::env::temp_dir().join(format!("kinetto-close-named-{}",std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for (case,save,failure) in [("save",true,false),("discard",false,false),("failure",true,true)] {
        let path = directory.join(format!("{case}.kinetto"));
        let mut h = Harness::builder().with_size([1100.0,800.0]).build_eframe(|cc| KinettoApp::new(cc));
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.state_mut().session = kn_engine::Session::new(kn_model::Project::sample());
        h.state_mut().choosing_template = false;
        h.state_mut().session.path = Some(path.clone());
        h.state_mut().write_file().unwrap();
        let original = h.state().project().clone();
        h.state_mut().session.execute(kn_engine::Command::SetDuration { index:0,millis:8000 }).unwrap();
        if failure { h.state_mut().session.path = Some(directory.join("missing").join("video.kinetto")); }
        h.state_mut().request_close();
        h.run_steps(4);
        assert!(!h.state().close_confirmed);
        h.get_by_label(if save { "Save and close" } else { "Discard" }).click();
        h.run_steps(4);
        assert_eq!(h.state().close_confirmed,!failure);
        if failure {
            assert!(h.state().problem.is_some());
            assert!(h.state().session.is_dirty());
        } else {
            eframe::App::on_exit(h.state_mut());
            let saved = storage::load(&path).unwrap();
            assert_eq!(saved.project.slides[0].duration,if save { kn_time::Tick::from_millis(8000) } else { original.slides[0].duration });
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn picture_slides_render_in_timeline() {
    let Some(dir) = std::env::var_os("KINETTO_PICTURE_REVIEW").map(std::path::PathBuf::from) else { return };
    let mut h = Harness::builder().with_size([1280.0,800.0]).wgpu().build_eframe(|cc| KinettoApp::new(cc));
    h.state_mut().recovery = None;
    h.state_mut().restorable = None;
    h.run_steps(5);
    let mut media = kn_media::sample_library();
    let mut outline = kn_import::outline_from_text("example\n\nexample\n1\n\nexample\n2",&mut media);
    for i in 0..12 { outline.parts.push(kn_import::Part::Picture { media:kn_media::sample_id(i%3),width:1600,height:1200,caption:String::new() }); }
    let bundle = storage::Bundle { project:kn_import::build(&outline,kn_geom::Aspect::Wide16x9,kn_layout::theme("Forest")).unwrap(),media };
    h.state_mut().replace_session(kn_engine::Session::from_bundle(bundle,None));
    h.state_mut().choosing_template = false;
    h.state_mut().settings.layout = super::settings::Layout::Timeline;
    h.state_mut().slide = 4;
    let time = h.state().slide_start(4)+kn_time::Tick::from_millis(600);
    h.state_mut().scrub = Some(time);
    h.run_steps(5);
    // Scrub over an entrance's background-only frame, then return to the picture.
    h.render().unwrap();
    h.state_mut().slide = 0;
    h.state_mut().scrub = Some(kn_time::Tick::ZERO);
    h.run_steps(4);
    h.render().unwrap();
    h.state_mut().slide = 4;
    h.state_mut().scrub = Some(time);
    h.run_steps(4);
    std::fs::create_dir_all(&dir).unwrap();
    let image = h.render().unwrap();
    let bg = h.state().project().slides[4].background;
    let visible = (180..750).flat_map(|x| (180..400).map(move |y| (x,y))).filter(|&(x,y)| {
        let pixel = image.get_pixel(x,y);
        pixel[0].abs_diff(bg.r)>12 || pixel[1].abs_diff(bg.g)>12 || pixel[2].abs_diff(bg.b)>12
    }).count();
    assert!(visible > 20000,"picture vanished after scrubbing: {visible} visible pixels");
    image.save(dir.join("timeline-picture.png")).unwrap();
}

#[test]
fn quick_creation_and_templates_include_their_media_and_sound() {
    let screenshots = std::env::var_os("KINETTO_CREATION_SCREENS").map(std::path::PathBuf::from);
    for (theme,name) in [(egui::Theme::Light,"light"),(egui::Theme::Dark,"dark")] {
        let builder = Harness::builder().with_size([1280.0,960.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.state_mut().settings = super::settings::Settings::default();
        h.run_steps(4);
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("welcome-{name}.png"))).unwrap(); }
        h.get_by_label("Try a sample").click();
        h.run_steps(4);
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("quick-{name}.png"))).unwrap(); }
        h.get_by_label("Free sample artwork").scroll_to_me();
        h.run_steps(8);
        h.get_by_label("Free sample artwork").click();
        h.run_steps(12);
        h.get_by_label("Add sample: Studio garden").scroll_to_me();
        h.run_steps(8);
        h.get_by_label("Add sample: Studio garden").click();
        h.run_steps(4);
        h.set_size(egui::vec2(760.0,850.0));
        h.run_steps(4);
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("quick-narrow-{name}.png"))).unwrap(); }
        h.get_by_label("Continue").click();
        h.run_steps(4);
        h.get_by_label("Back").click();
        h.run_steps(4);
        h.get_by_label("Continue").click();
        h.run_steps(4);
        h.get_by_label("Continue").click();
        h.run_steps(4);
        h.get_by_label("Create video").click();
        h.run_steps(4);
        assert!(!h.state().choosing_template);
        let project = h.state().project();
        assert!(project.music.is_some());
        assert!(project.media_ids().count() >= 2);
        for id in project.media_ids() { assert!(h.state().session.media.decode(id).is_ok()); }
        h.set_size(egui::vec2(1280.0,960.0));
        h.state_mut().choosing_template = true;
        h.run_steps(4);
        h.get_by_role_and_label(egui::Role::RadioButton, "Business").click();
        h.run_steps(4);
        h.get_by_label("Product promo").click();
        h.run_steps(4);
        assert!(!h.state().choosing_template);
        assert!(h.state().project().music.is_some());
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("template-editor-{name}.png"))).unwrap(); }
        let path = std::env::temp_dir().join(format!("kinetto-template-{}-{name}.kinetto",std::process::id()));
        storage::save(h.state().project(),&h.state().session.media,&path).unwrap();
        let loaded = storage::load(&path).unwrap();
        for id in loaded.project.media_ids() { assert!(loaded.media.decode(id).is_ok()); }
        assert_eq!(&loaded.project,h.state().project());
        std::fs::remove_file(path).unwrap();
        h.get_by_label("Quick video").click();
        h.run_steps(4);
        assert!(h.state().quick.is_some());
        h.get_by_label("Cancel").click();
        h.run_steps(4);
        assert!(h.state().quick.is_none());
        assert_eq!(&loaded.project,h.state().project());
    }
}

#[test]
fn slide_look_picker_changes_one_slide_then_applies_to_all() {
    let screenshots = std::env::var_os("KINETTO_SLIDE_SCREENS").map(std::path::PathBuf::from);
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let builder = Harness::builder().with_size([1100.0, 900.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        let project = kn_model::Project::sample();
        let second = project.slides[1].clone();
        open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Simple));
        h.state_mut().selected = None;
        h.run_steps(4);
        h.get_by_role_and_label(egui::Role::RadioButton, "Ocean").click();
        h.run_steps(4);
        assert_eq!(h.state().project().slides[0].theme.as_deref(), Some("Ocean"));
        assert_eq!(h.state().project().slides[1], second);
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("slide-inspector-{name}.png"))).unwrap(); }
        h.get_by_label("Apply to all slides").click();
        h.run_steps(4);
        assert_eq!(h.state().project().theme, "Ocean");
        assert_eq!(h.state().project().slides[1].background, kn_layout::theme("Ocean").background);
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project().slides[1], second);
    }
}

#[test]
fn photo_motion_presets_apply_and_undo_in_both_themes() {
    let screenshots = std::env::var_os("KINETTO_PICTURE_SCREENS").map(std::path::PathBuf::from);
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let builder = Harness::builder().with_size([1100.0, 900.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        let mut project = kn_model::Project::sample();
        project.slides[0].elements = vec![kn_model::Element::image("test-photo".into(), kn_geom::Rect::new(100., 100., 600., 600.))];
        open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Simple));
        h.state_mut().selected = Some(0);
        h.run_steps(4);
        h.get_by_label("Photo preset").scroll_to_me();
        h.run_steps(4);
        h.get_by_label("Photo preset").click();
        h.run_steps(4);
        h.get_by_label("Product focus").click();
        h.run_steps(4);
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("picture-inspector-{name}.png"))).unwrap(); }
        let element = &h.state().project().slides[0].elements[0];
        assert_eq!(element.photo_motion, kn_model::PhotoMotion::ZoomIn);
        assert_eq!(element.photo_amount, 0.16);
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project().slides[0].elements[0].photo_motion, kn_model::PhotoMotion::None);
    }
}

#[test]
fn text_motion_picker_updates_timing_and_undo_in_both_themes() {
    let screenshots = std::env::var_os("KINETTO_MOTION_SCREENS").map(std::path::PathBuf::from);
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let builder = Harness::builder().with_size([1100.0, 900.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
        h.state_mut().selected = Some(1);
        h.run_steps(4);
        h.get_by_label("Text animation").click();
        h.run_steps(4);
        h.get_by_label("Entrance animation").scroll_to_me();
        h.run_steps(4);
        h.get_by_label("Entrance animation").click();
        h.run_steps(4);
        h.get_by_label("Letter by letter").click();
        h.run_steps(4);
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("text-motion-menu-{name}.png"))).unwrap(); }
        h.get_by_label("Draw letters").click();
        h.run_steps(4);
        let element = &h.state().project().slides[0].elements[1];
        assert_eq!(element.motion, kn_model::Motion::LetterReveal);
        assert_eq!(element.motion_duration_ms, 1800);
        assert_eq!(element.easing, kn_model::Easing::Linear);
        assert!(!h.state().playback.playing);
        assert_eq!(h.state().selected, Some(1));
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("text-motion-{name}.png"))).unwrap(); }
        h.get_by_label("Replay").scroll_to_me();
        h.run_steps(4);
        h.get_by_label("Replay").click();
        h.run_steps(4);
        assert!(h.state().playback.playing);
        h.state_mut().stop_playback();
        assert!(h.state_mut().session.undo());
        let element = &h.state().project().slides[0].elements[1];
        assert_eq!(element.motion, kn_model::Motion::Rise);
        assert_eq!(element.motion_duration_ms, 600);
    }
}

#[test]
fn shape_gallery_inserts_selected_geometry_and_supports_undo() {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut h = Harness::builder().with_size([900.0, 700.0]).with_theme(theme).build_eframe(|cc| KinettoApp::new(cc));
        open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), None);
        h.state_mut().selected = None;
        h.run_steps(4);
        let before = h.state().project().slides[0].elements.len();
        h.get_by_label("Add text, a shape or a picture").click();
        h.run_steps(4);
        h.get_by_label("Star").click();
        h.run_steps(4);
        let elements = &h.state().project().slides[0].elements;
        assert_eq!(elements.len(), before + 1);
        assert!(matches!(elements.last().unwrap().kind, kn_model::ElementKind::Shape { shape: kn_model::ShapeKind::Star, .. }));
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project().slides[0].elements.len(), before);
    }
}

#[test]
fn square_rotation_preserves_geometry_and_undo() {
    let screenshots = std::env::var_os("KINETTO_SHAPE_SCREENS").map(std::path::PathBuf::from);
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let builder = Harness::builder().with_size([1100.0, 900.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
        h.state_mut().add_shape(kn_model::ShapeKind::Square);
        h.run_steps(4);
        let index = h.state().selected.unwrap();
        let before = h.state().project().slides[0].elements[index].clone();
        assert_eq!(before.frame.w, before.frame.h);
        h.get_by_role_and_label(egui::Role::Slider, "Shape rotation").set_value_accesskit(37.5);
        h.run_steps(4);
        let rotated = &h.state().project().slides[0].elements[index];
        assert_eq!(rotated.rotation, 37.5);
        assert_eq!(rotated.frame, before.frame);
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("rotated-square-{name}.png"))).unwrap(); }
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project().slides[0].elements[index], before);
        h.run_steps(4);
        h.get_by_role_and_label(egui::Role::Slider, "Shape rotation").set_value_accesskit(37.5);
        h.run_steps(4);
        h.get_by_label("Reset rotation").click();
        h.run_steps(4);
        assert_eq!(h.state().project().slides[0].elements[index].rotation, 0.);
    }
}

#[test]
fn timeline_switches_between_slides_with_different_item_counts() {
    let mut h = Harness::builder().with_size([1280.0, 800.0]).build_eframe(|cc| KinettoApp::new(cc));
    let mut project = kn_model::Project::sample();
    project.slides[1].elements.clear();
    open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Timeline));
    h.run_steps(4);
    for (label, index) in [("Slide 2", 1), ("Slide 1", 0), ("Slide 2", 1)] {
        h.get_by_label(label).click();
        h.run_steps(4);
        assert_eq!(h.state().slide, index);
    }
}

#[test]
fn problem_dialog_closes_with_escape_or_enter_and_keeps_the_slide() {
    let mut h = Harness::builder().with_size([1100.0, 800.0]).build_eframe(|cc| KinettoApp::new(cc));
    open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
    h.state_mut().selected = Some(1);
    let before = h.state().project().clone();
    for key in [egui::Key::Escape, egui::Key::Enter] {
        h.state_mut().report(Problem::new("Something went wrong", "Try again."));
        h.run_steps(4);
        h.key_press(egui::Key::Delete);
        h.run_steps(2);
        assert!(h.state().problem.is_some(), "keys behind a dialog must not reach the editor");
        h.key_press(key);
        h.run_steps(4);
        assert!(h.state().problem.is_none(), "{key:?} should close the dialog");
        assert_eq!(h.state().selected, Some(1), "{key:?} must not also deselect the item");
    }
    assert_eq!(h.state().project(), &before);
}

#[test]
fn focused_slider_arrow_keys_change_the_value_without_moving_the_item() {
    let mut h = Harness::builder().with_size([1100.0, 900.0]).build_eframe(|cc| KinettoApp::new(cc));
    open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
    h.state_mut().selected = Some(1);
    h.run_steps(4);
    let before = h.state().project().slides[0].elements[1].clone();
    h.get_by_role_and_label(egui::Role::Slider, "Size").focus();
    h.run_steps(2);
    // Pressed twice: focus must stay on the slider, not move to a neighbour.
    for _ in 0..2 {
        h.key_press(egui::Key::ArrowRight);
        h.run_steps(4);
    }
    let after = &h.state().project().slides[0].elements[1];
    assert_eq!(after.frame, before.frame);
    let size = |e: &kn_model::Element| match e.kind { kn_model::ElementKind::Text { size, .. } => size, _ => unreachable!() };
    assert_eq!(size(after), size(&before) + 2.0);
}

#[test]
fn focused_rotation_knob_keeps_the_arrow_keys() {
    // The stage (and its knob) needs the GPU canvas.
    if kn_export::Gpu::create_for_tests().is_none() {
        eprintln!("no GPU; skipping");
        return;
    }
    let mut h = Harness::builder().with_size([1100.0, 900.0]).wgpu().build_eframe(|cc| KinettoApp::new(cc));
    open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
    h.state_mut().add_shape(kn_model::ShapeKind::Square);
    h.run_steps(4);
    let index = h.state().selected.unwrap();
    h.get_by_label("Rotate selected object").focus();
    h.run_steps(2);
    // Each press turns the shape; focus must not wander to the sidebar.
    for _ in 0..3 {
        h.key_press(egui::Key::ArrowLeft);
        h.run_steps(4);
    }
    assert_eq!(h.state().project().slides[0].elements[index].rotation, -3.0);
}

#[test]
fn dragging_a_sidebar_slide_moves_it_in_one_undo_step_and_escape_cancels() {
    let screenshots = std::env::var_os("KINETTO_DRAG_SCREENS").map(std::path::PathBuf::from);
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let builder = Harness::builder().with_size([1200.0, 1100.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        // Four slides, told apart by their lengths (2 s to 5 s).
        let mut project = kn_model::Project::sample();
        let first = project.slides[0].clone();
        project.slides = (2..6).map(|secs| kn_model::Slide { duration: kn_time::Tick::from_secs(secs), ..first.clone() }).collect();
        open_editor(&mut h, kn_engine::Session::new(project.clone()), Some(super::settings::Layout::Simple));
        h.state_mut().playback.player = None;
        h.run_steps(4);
        // Drag slide 1 to just under the middle of slide 3.
        let from = h.get_by_role_and_label(egui::Role::ListItem, "Slide 1").rect().center();
        let third = h.get_by_role_and_label(egui::Role::ListItem, "Slide 3").rect();
        let to = egui::pos2(from.x, third.center().y + third.height() * 0.3);
        h.drag_at(from);
        h.run_steps(2);
        h.hover_at(egui::pos2(from.x, from.y + 30.0));
        h.run_steps(2);
        h.hover_at(to);
        h.run_steps(4);
        assert_eq!(h.state().slide_drag, Some(0));
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("slide-drag-{name}.png"))).unwrap(); }
        h.drop_at(to);
        h.run_steps(4);
        assert!(h.state().slide_drag.is_none());
        let order: Vec<_> = h.state().project().slides.iter().map(|s| project.slides.iter().position(|o| o == s).unwrap()).collect();
        assert_eq!(order, [1, 2, 0, 3]);
        assert_eq!(h.state().slide, 2, "the moved slide stays selected");
        assert!(h.state_mut().session.undo());
        assert_eq!(h.state().project(), &project, "one undo puts it back");

        // Escape mid-drag leaves the order alone.
        h.run_steps(4);
        let from = h.get_by_role_and_label(egui::Role::ListItem, "Slide 2").rect().center();
        let last = h.get_by_role_and_label(egui::Role::ListItem, "Slide 4").rect();
        h.drag_at(from);
        h.run_steps(2);
        h.hover_at(egui::pos2(from.x, last.bottom()));
        h.run_steps(4);
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        h.drop_at(egui::pos2(from.x, last.bottom()));
        h.run_steps(4);
        assert_eq!(h.state().project(), &project);

        // The pointer leaves and the window loses focus mid-drag (say,
        // alt-tab): the button is up with no pointer position. The drag
        // ends without a move, and coming back in later drops nothing.
        h.drag_at(from);
        h.run_steps(2);
        h.hover_at(egui::pos2(from.x, last.bottom()));
        h.run_steps(4);
        assert!(h.state().slide_drag.is_some());
        h.event(egui::Event::PointerGone);
        h.event(egui::Event::WindowFocused(false));
        h.run_steps(4);
        assert!(h.state().slide_drag.is_none(), "losing focus must end the drag");
        h.event(egui::Event::WindowFocused(true));
        h.hover_at(egui::pos2(from.x, last.bottom()));
        h.run_steps(4);
        h.drop_at(egui::pos2(from.x, last.bottom()));
        h.run_steps(4);
        assert!(h.state().slide_drag.is_none());
        assert_eq!(h.state().project(), &project, "nothing moves after coming back");
        assert_eq!(h.state().project(), &project);
    }
}

#[test]
fn template_categories_filter_the_welcome_page_and_open_a_template() {
    let screenshots = std::env::var_os("KINETTO_TEMPLATE_SCREENS").map(std::path::PathBuf::from);
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let builder = Harness::builder().with_size([1440.0, 1000.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.run_steps(4);
        // "All" groups every template under its category's heading.
        // Once as a chip, once as the heading over its templates.
        assert_eq!(h.query_all_by_label("Memories & collages").count(), 2);
        assert!(h.query_by_label("Product promo").is_some());
        h.get_by_role_and_label(egui::Role::RadioButton, "Celebrations").click();
        h.run_steps(4);
        assert_eq!(h.state().template_category, Some(kn_layout::Category::Celebrations));
        assert!(h.query_by_label("Product promo").is_none());
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("templates-celebrations-{name}.png"))).unwrap(); }
        h.get_by_label("Birthday").click();
        h.run_steps(4);
        assert!(!h.state().choosing_template);
        assert_eq!(h.state().project(), &kn_layout::template(kn_layout::Template::Birthday));
        for id in h.state().project().media_ids() { assert!(h.state().session.media.decode(id).is_ok()); }
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("birthday-editor-{name}.png"))).unwrap(); }
    }
}

#[test]
fn long_lists_scroll_inside_themselves_so_what_follows_stays_close() {
    let screenshots = std::env::var_os("KINETTO_SCROLL_SCREENS").map(std::path::PathBuf::from);
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let builder = Harness::builder().with_size([1200.0, 900.0]).with_theme(theme);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        // Recent projects: twenty of them still leave the templates close.
        let recent: Vec<std::path::PathBuf> = (0..20).map(|i| std::env::temp_dir().join(format!("kinetto-missing-{i}.kinetto"))).collect();
        h.state_mut().settings.recent_projects = recent;
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.run_steps(4);
        let gap = h.get_by_label("Templates").rect().top() - h.get_by_label("Recent projects").rect().top();
        assert!(gap < super::recent::RECENT_HEIGHT + 140.0, "recent list grew to {gap}");
        let first = h.get_by_label("Open kinetto-missing-0").rect();
        let third = h.get_by_label("Open kinetto-missing-2").rect();
        let templates = h.get_by_label("Templates").rect();
        assert!(third.bottom() > first.bottom() && third.bottom() < templates.top(), "at least three recent projects must fit before templates");
        assert!(gap >= super::recent::RECENT_HEIGHT, "recent viewport must not collapse to the remaining page space");

        // Thirty items on a slide.
        let mut project = kn_model::Project::sample();
        let item = |i: usize| kn_model::Element::text(&format!("Item {i}"), kn_geom::Rect::new(100.0, 100.0, 400.0, 80.0), 40.0, false, kn_geom::Color::rgb(0, 0, 0));
        project.slides[0].elements = (0..30).map(item).collect();
        open_editor(&mut h, kn_engine::Session::new(project), Some(super::settings::Layout::Simple));
        h.state_mut().selected = None;
        h.run_steps(4);
        let gap = h.get_by_label("Timing").rect().top() - h.get_by_label("Items on this slide").rect().top();
        assert!(gap < super::inspector::ITEMS_LIST_HEIGHT + 160.0, "items list grew to {gap}");
        if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("items-scroll-{name}.png"))).unwrap(); }
        // The last row starts out of view, below the list's bottom edge...
        let timing = h.get_by_label("Timing").rect().top();
        assert!(h.get_by_label("Item 0").rect().top() > timing, "Item 0 should start scrolled out of view");
        // Turning the mouse wheel over the list scrolls the list first; the
        // panel stays put until the list reaches its end.
        let wheel = |h: &mut Harness<'_, KinettoApp>| {
            h.event(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, -150.0), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: egui::Modifiers::NONE });
            h.run_steps(12);
        };
        h.hover_at(h.get_by_label("Item 26").rect().center());
        h.run_steps(2);
        let top_row = h.get_by_label("Item 29").rect().top();
        wheel(&mut h);
        assert!(h.get_by_label("Item 29").rect().top() < top_row - 100.0, "the list scrolled");
        assert_eq!(h.get_by_label("Timing").rect().top(), timing, "the panel stayed put");
        // Scrolling on brings the last row into view inside the list.
        for _ in 0..12 {
            if h.get_by_label("Item 0").rect().bottom() < h.get_by_label("Timing").rect().top() { break; }
            wheel(&mut h);
        }
        assert!(h.get_by_label("Item 0").rect().bottom() < h.get_by_label("Timing").rect().top(), "Item 0 scrolled into view");
        h.get_by_label("Item 0").click();
        h.run_steps(4);
        assert_eq!(h.state().selected, Some(0));

        // A long text scrolls inside the Words box.
        let long = (0..60).map(|i| format!("Line {i}")).collect::<Vec<_>>().join("\n");
        h.state_mut().session.execute(kn_engine::Command::UpdateElement { slide: 0, index: 0, element: kn_model::Element::text(&long, kn_geom::Rect::new(100.0, 100.0, 400.0, 80.0), 40.0, false, kn_geom::Color::rgb(0, 0, 0)) }).unwrap();
        h.run_steps(4);
        let gap = h.get_by_role_and_label(egui::Role::Slider, "Size").rect().top() - h.get_by_label("Words").rect().top();
        assert!(gap < super::inspector::WORDS_HEIGHT + 120.0, "words box grew to {gap}");
        // The text's last lines start below the box; the wheel over the
        // box scrolls the text itself until its end comes into view.
        let size_row = h.get_by_role_and_label(egui::Role::Slider, "Size").rect().top();
        let text = |h: &Harness<'_, KinettoApp>| h.get_by_role(egui::Role::MultilineTextInput).rect();
        assert!(text(&h).bottom() > size_row, "the long text should overflow the box");
        let start = text(&h).top();
        h.hover_at(text(&h).center().min(egui::pos2(f32::INFINITY, size_row - 40.0)));
        h.run_steps(2);
        wheel(&mut h);
        assert!(text(&h).top() < start - 100.0, "the text scrolled");
        assert_eq!(h.get_by_role_and_label(egui::Role::Slider, "Size").rect().top(), size_row, "the panel stayed put");
        for _ in 0..20 {
            if text(&h).bottom() <= size_row { break; }
            wheel(&mut h);
        }
        assert!(text(&h).bottom() <= size_row, "Line 59, the end of the text, scrolled into view");
    }
}

#[test]
fn sidebar_drag_moves_nothing_when_let_go_away_from_the_list_or_after_an_undo() {
    let mut h = Harness::builder().with_size([1200.0, 1100.0]).build_eframe(|cc| KinettoApp::new(cc));
    let mut project = kn_model::Project::sample();
    let first = project.slides[0].clone();
    project.slides = (2..6).map(|secs| kn_model::Slide { duration: kn_time::Tick::from_secs(secs), ..first.clone() }).collect();
    open_editor(&mut h, kn_engine::Session::new(project.clone()), Some(super::settings::Layout::Simple));
    h.state_mut().playback.player = None;
    h.run_steps(4);
    let thumb = |h: &Harness<'_, KinettoApp>, n: usize| h.get_by_role_and_label(egui::Role::ListItem, &format!("Slide {n}")).rect();

    // Dragged out over the stage and let go there: nothing moves.
    let from = thumb(&h, 1).center();
    let stage = egui::pos2(700.0, thumb(&h, 4).center().y);
    h.drag_at(from);
    h.run_steps(2);
    h.hover_at(egui::pos2(from.x, from.y + 30.0));
    h.run_steps(2);
    h.hover_at(stage);
    h.run_steps(4);
    assert!(h.state().slide_drag.is_some());
    h.drop_at(stage);
    h.run_steps(4);
    assert!(h.state().slide_drag.is_none());
    assert_eq!(h.state().project(), &project, "let go over the stage, the order stays");

    // An undo mid-drag changes the slides under it: the drag is cancelled
    // and letting go over the list moves nothing.
    h.state_mut().session.execute(kn_engine::Command::SetDuration { index: 3, millis: 9000 }).unwrap();
    h.state_mut().session.seal();
    h.run_steps(4);
    let from = thumb(&h, 1).center();
    let below_third = egui::pos2(from.x, thumb(&h, 3).bottom());
    h.drag_at(from);
    h.run_steps(2);
    h.hover_at(below_third);
    h.run_steps(4);
    assert!(h.state().slide_drag.is_some());
    assert!(h.state_mut().session.undo());
    h.run_steps(2);
    assert!(h.state().slide_drag.is_none(), "an edit mid-drag cancels it");
    h.drop_at(below_third);
    h.run_steps(4);
    assert_eq!(h.state().project(), &project, "only the undo happened");
}

#[test]
fn render_screens() {
    let Some(dir) = std::env::var_os("KINETTO_SCREENS").map(std::path::PathBuf::from) else { return };
    // Keep the safety copy away from the real one in the user's data folder.
    let data = std::env::temp_dir().join(format!("kinetto-screens-{}", std::process::id()));
    for (theme, name) in [(egui::Theme::Light, "light"), (egui::Theme::Dark, "dark")] {
        let mut h = Harness::builder()
            .with_size([1280.0, 800.0])
            .with_theme(theme)
            .wgpu()
            .build_eframe(|cc| KinettoApp::new(cc));
        h.state_mut().recovery = Some(storage::Recovery::at(data.clone()));
        h.state_mut().restorable = None;
        // Not the user's saved preference: each screen sets its own.
        h.state_mut().settings = super::settings::Settings::default();
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("welcome-{name}.png"))).unwrap();
        h.get_by_label("Main menu").click();
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("menu-{name}.png"))).unwrap();
        h.key_press(egui::Key::Escape);
        h.run_steps(4);

        // With the restore banner showing, nothing inside the window may
        // be see-through (the window is transparent for its corners).
        h.state_mut().restorable = Some(storage::Bundle { project: kn_model::Project::sample(), media: kn_media::MediaLibrary::new() });
        h.run_steps(4);
        let img = h.render().unwrap();
        img.save(dir.join(format!("banner-{name}.png"))).unwrap();
        let (w, h_px) = img.dimensions();
        let corner = 14;
        let see_through = img
            .enumerate_pixels()
            .filter(|(x, y, _)| {
                let near_x = *x < corner || *x >= w - corner;
                let near_y = *y < corner || *y >= h_px - corner;
                !(near_x && near_y)
            })
            .filter(|(_, _, px)| px.0[3] < 255)
            .count();
        assert_eq!(see_through, 0, "{see_through} see-through pixels in the {name} window");
        h.state_mut().restorable = None;
        // Let the page settle without the banner before clicking.
        h.run_steps(4);

        h.get_by_role_and_label(egui::Role::RadioButton, "Business").click();
        h.run_steps(4);
        h.get_by_label("Product promo").click();
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("editor-{name}.png"))).unwrap();

        h.get_by_label("Animation between slides 1 and 2").click();
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("transition-menu-{name}.png"))).unwrap();
        h.key_press(egui::Key::Escape);
        h.run_steps(4);

        h.state_mut().selected = Some(1);
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("item-{name}.png"))).unwrap();
        // Allow a focused shell review without opening unrelated dialogs.
        if std::env::var_os("KINETTO_EDITOR_SCREENS_ONLY").is_some() { continue; }

        h.state_mut().settings.layout = super::settings::Layout::Timeline;
        h.state_mut().slide = 2;
        h.state_mut().selected = Some(3);
        // A real song, placed from 3 s to 14 s and starting 1.5 s in.
        let mut song = kn_audio::generate(kn_model::Mood::Upbeat, 48_000 * 10);
        kn_audio::loudness::normalise(&mut song.samples);
        let id = h.state_mut().session.media.import_audio(song.to_wav()).unwrap();
        let mut music = kn_model::Music::new(kn_model::MusicSource::File(id), 0.6);
        (music.start_ms, music.end_ms, music.offset_ms) = (3000, Some(14_000), 1500);
        h.state_mut().session.execute(kn_engine::Command::SetMusic { music: Some(music) }).unwrap();
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("timeline-{name}.png"))).unwrap();
        h.state_mut().selected = None;
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("timeline-slide-{name}.png"))).unwrap();
        h.state_mut().slide = 1;
        h.state_mut().scrub = None;
        h.set_size(egui::vec2(1280.0, 960.0));
        h.run_steps(6);
        h.render().unwrap().save(dir.join(format!("timeline-music-{name}.png"))).unwrap();
        h.set_size(egui::vec2(1280.0, 800.0));
        h.state_mut().settings.layout = super::settings::Layout::Simple;

        let mut quick = super::quick::Quick::new("Our trip\nTwo weeks in Japan\n\nWhat we saw\n- Temples\n- Mountains\n- Ramen".into());
        for (pw, ph) in [(640, 480), (480, 640), (800, 800)] {
            quick.add_picture(&h.ctx, kn_media::test_png(pw, ph), "a test picture");
        }
        h.state_mut().quick = Some(quick);
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("paste-{name}.png"))).unwrap();
        h.set_size(egui::vec2(1280.0, 1100.0));
        h.state_mut().settings.quick.style.motion = kn_import::Pick::Random;
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("quick-tall-{name}.png"))).unwrap();
        h.set_size(egui::vec2(1280.0, 800.0));
        h.run_steps(2);
        // The wizard opens on its first page; Create is on the last.
        for _ in 0..2 {
            h.get_by_label("Continue").click();
            h.run_steps(4);
        }
        h.get_by_label("Create video").click();
        h.run_steps(4);
        h.get_by_label("Discard").click();
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("imported-{name}.png"))).unwrap();

        let full = std::io::Error::from(std::io::ErrorKind::StorageFull);
        h.state_mut().save_trouble = Some(format!("Changes can't be saved to \u{201c}Talk.kinetto\u{201d}. {}", problem::io_reason(&full)));
        h.state_mut().report(Problem::save(std::path::Path::new("/home/me/Talk.kinetto"), &storage::StorageError::Io(full)));
        h.run_steps(4);
        h.render().unwrap().save(dir.join(format!("problem-{name}.png"))).unwrap();
    }
    let _ = std::fs::remove_dir_all(data);
}

/// Every language draws the welcome page, the editor and the quick dialog
/// without panicking; widgets answer to their translated names, and Hebrew
/// swaps the side panels.
#[test]
fn every_language_translates_the_editor_and_hebrew_mirrors_it() {
    use super::i18n::Language;
    let screenshots = std::env::var_os("KINETTO_SCREENS").map(std::path::PathBuf::from);
    for (language, code, export, slide_settings) in [
        (Language::Hebrew, "he", "ייצוא", "הגדרות השקף"),
        (Language::Russian, "ru", "Экспорт", "Настройки слайда"),
        (Language::Chinese, "zh", "导出", "幻灯片设置"),
        (Language::French, "fr", "Exporter", "Réglages de la diapositive"),
        (Language::Spanish, "es", "Exportar", "Ajustes de la diapositiva"),
        (Language::German, "de", "Exportieren", "Folieneinstellungen"),
    ] {
        let builder = Harness::builder().with_size([1280.0, 800.0]);
        let builder = if screenshots.is_some() { builder.wgpu() } else { builder };
        let mut h = builder.build_eframe(|cc| KinettoApp::new(cc));
        h.state_mut().recovery = None;
        h.state_mut().restorable = None;
        h.state_mut().settings = super::settings::Settings::default();
        h.state_mut().settings.language = language;
        h.run_steps(4);
        let shot = |h: &mut Harness<'_, KinettoApp>, name: &str| {
            if let Some(dir) = &screenshots { h.render().unwrap().save(dir.join(format!("lang-{code}-{name}.png"))).unwrap(); }
        };
        shot(&mut h, "welcome");
        let ctx = h.state().ctx.clone();
        h.state_mut().open_quick_sample(&ctx);
        h.run_steps(4);
        shot(&mut h, "quick");
        h.state_mut().quick = None;
        open_editor(&mut h, kn_engine::Session::new(kn_model::Project::sample()), Some(super::settings::Layout::Simple));
        h.run_steps(4);
        // Widgets are named by the text they show, which for Hebrew is in display order.
        use super::i18n::{tr, written};
        assert_eq!((&*written(tr("Export")), &*written(tr("Slide settings"))), (export, slide_settings));
        assert!(h.query_by_label(tr("Export")).is_some(), "{language:?}: no Export button");
        let settings = h.get_by_label(tr("Slide settings")).rect().center().x;
        let width = 1280.0;
        if language.is_rtl() {
            assert!(settings < width / 2.0, "Hebrew puts the settings panel on the left");
        } else {
            assert!(settings > width / 2.0, "{language:?} keeps the settings panel on the right");
        }
        shot(&mut h, "editor");
        h.state_mut().selected = Some(1);
        h.run_steps(4);
        shot(&mut h, "item");
        h.state_mut().selected = None;
        h.state_mut().settings.layout = super::settings::Layout::Timeline;
        h.run_steps(4);
        shot(&mut h, "timeline");
    }
    super::i18n::set(Language::English);
}

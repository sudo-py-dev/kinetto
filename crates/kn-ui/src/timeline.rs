//! The timeline layout: the whole video laid out in time under the slide.
//! Slides are blocks (drag to reorder, drag the right edge to change their
//! length), the current slide's items are bars (drag to change when they
//! arrive), sound has its own row, and the ruler scrubs.

use crate::i18n::tr;
use crate::KinettoApp;
use crate::adw::{self, Kind};
use crate::canvas::{Frame, Surface};
use crate::icons::{self, Icon};
use crate::theme::{self, palette};
use egui::{Align2, Color32, CursorIcon, FontId, Rect, Sense, Stroke, Vec2, pos2, vec2};
use kn_engine::Command;
use kn_model::{ ElementKind, Motion, MusicSource, Transition};
use kn_time::Tick;

/// Starting height of the timeline panel.
pub const DEFAULT_HEIGHT: f32 = 380.0;
/// Width of the label column on the left.
const LABELS: f32 = 176.0;
/// Height of the time ruler.
const RULER: f32 = 24.0;
/// Height of the slides row.
const SLIDE_ROW: f32 = 58.0;
/// Height of each item row.
const ROW: f32 = 28.0;
/// Height of the music row.
const MUSIC_ROW: f32 = 72.0;
/// Height of the voice row.
const VOICE_ROW: f32 = 56.0;
/// Width of a slide block's draggable right edge.
const EDGE: f32 = 7.0;
/// Points per second when the user hasn't zoomed.
const DEFAULT_ZOOM: f32 = 60.0;
/// Space left on the toolbar before the drag hint is shown: its own width
/// plus the New slide button beside it.
const HINT_ROOM: f32 = 400.0;
/// Shortest a slide can be dragged to.
const MIN_SLIDE_MS: i64 = 1000;
/// Longest a slide can be dragged to.
const MAX_SLIDE_MS: i64 = 20_000;

/// A drag in progress on the timeline. The whole drag is one undo step,
/// sealed when the button is released.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TimelineDrag {
    /// Moving the playhead along the ruler.
    Scrub,
    /// Changing a slide's length from its right edge.
    Resize { slide: usize, start_x: f32 },
    /// Moving a slide to another place.
    Move { slide: usize },
    /// Changing when an item arrives.
    ItemStart { index: usize, origin_ms: i64, grab_x: f32 },
    /// Moving the music along the video, keeping its length.
    MusicMove { grab_x: f32, start_ms: i64, end_ms: i64, until_end: bool },
    /// Trimming the music's beginning: it starts later in the video and
    /// later in the song, so what plays at each moment stays the same.
    MusicStart { grab_x: f32, start_ms: i64, offset_ms: i64 },
    /// Moving where the music stops.
    MusicEnd { grab_x: f32, end_ms: i64 },
}

/// Waveform peaks per second of song.
const PEAKS_PER_SECOND: usize = 20;
/// A sound's loudness over time, for drawing.
pub(crate) struct WaveformData {
    /// Peak level per [`PEAKS_PER_SECOND`] slice, 0 to 1.
    peaks: Vec<f32>,
    /// Length of the sound.
    seconds: f64,
}

/// A waveform for one sound, worked out off the UI thread.
pub(crate) enum Waveform {
    /// Still being analysed.
    Loading(std::sync::mpsc::Receiver<WaveformData>),
    /// Ready to draw.
    Ready(std::sync::Arc<WaveformData>),
    /// The sound couldn't be decoded.
    Unavailable,
}

/// Edges and slide boundaries pull the music this close, in points.
const SNAP: f32 = 8.0;

/// `ms`, moved onto the nearest of `targets` when within [`SNAP`] points.
fn snap(ms: i64, targets: &[i64], zoom: f32) -> i64 {
    let reach = (f64::from(SNAP / zoom) * 1000.0) as i64;
    targets.iter().copied().filter(|t| (t - ms).abs() <= reach).min_by_key(|t| (t - ms).abs()).unwrap_or(ms)
}

/// Page only when the head leaves the safe visible region. Keep it near
/// the left edge of the next page instead of chasing it a few pixels per frame.
fn follow_page(head: f32, left: f32, right: f32) -> Option<f32> {
    if !head.is_finite() || right - left < 64.0 { return None; }
    if head >= right - 16.0 || head < left { Some(head - 24.0) } else { None }
}

/// A short, recognisable name for an item.
pub(crate) fn item_name(kind: &ElementKind) -> (Icon, String) {
    match kind {
        ElementKind::Text { text, .. } => {
            let first = text.lines().next().unwrap_or("").trim();
            let short: String = first.chars().take(22).collect();
            let name = if short.is_empty() { tr("Text").into() } else if first.chars().count() > 22 { crate::i18n::display(&format!("{short}…")).into_owned() } else { crate::i18n::display(&short).into_owned() };
            (Icon::Text, name)
        }
        ElementKind::Shape { shape, .. } => (Icon::Shape, tr(shape.name()).into()),
        ElementKind::Image { .. } => (Icon::Picture, tr("Picture").into()),
    }
}

/// Seconds as `m:ss.s` for the time display.
fn clock(secs: f64) -> String {
    format!("{}:{:04.1}", (secs / 60.0) as u64, secs % 60.0)
}

impl KinettoApp {
    /// The Timeline layout's bottom panel: play controls and zoom, the labels
    /// column, and the scrollable tracks.
    pub(crate) fn timeline(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        if ui.input(|i| !i.focused || i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(false))) || (!i.pointer.primary_down() && !i.pointer.button_released(egui::PointerButton::Primary))) && self.timeline_drag.take().is_some() {
            self.session.seal();
        }
        let p = palette(ui.ctx());
        let mut zoom = self.settings.timeline_zoom.unwrap_or(DEFAULT_ZOOM);
        let total = self.project().total_duration().as_secs_f64();

        // Toolbar, on one row so the tracks get the height: play, time, zoom, new slide.
        // It follows the reading direction; the tracks below always run left to right.
        egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 4 }).show(ui, |ui| adw::reading_order(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon, tip) = if self.playback.playing { (Icon::Stop, "Stop  (Space)") } else { (Icon::Play, "Play from this slide  (Space)") };
                if adw::icon_button(ui, icon, tip).clicked() {
                    self.toggle_play();
                }
                if adw::icon_button(ui, Icon::Undo, "Restart video").clicked() {
                    self.scrub_to(0.0);
                }
                let now = self.playhead().as_secs_f64();
                ui.label(egui::RichText::new(format!("{}  /  {}", clock(now), clock(total))).family(theme::semibold()).size(13.0));
                ui.add_space(12.0);
                ui.label(egui::RichText::new(tr("Zoom")).size(12.5).color(p.dim));
                let mut z = f64::from(zoom);
                let r = adw::slider(ui, &mut z, 1.0..=240.0, 1.0, "Zoom");
                if r.changed() {
                    zoom = z as f32;
                    self.settings.timeline_zoom = Some(zoom);
                }
                if r.drag_stopped() || (r.changed() && !r.dragged()) { self.settings.save(); }
                if adw::button(ui, None, "Fit", Kind::Flat).on_hover_text(tr("Fit the whole video in the timeline")).clicked() {
                    zoom = ((ui.max_rect().width() - LABELS - 36.0) / total.max(0.001) as f32).clamp(1.0, 240.0);
                    self.settings.timeline_zoom = Some(zoom);
                    self.settings.save();
                }
                // The hint gives way first when the panel is narrow.
                if ui.available_width() > HINT_ROOM {
                    ui.label(egui::RichText::new(tr("Drag slides to reorder · Edges change length")).size(11.5).color(p.dim));
                }
                ui.with_layout(adw::trailing(), |ui| {
                    let response = adw::button(ui, Some(Icon::Add), "New slide", Kind::Normal);
                    egui::Popup::menu(&response).width(180.0).show(|ui| {
                        for layout in kn_layout::Layout::ALL {
                            if crate::header::menu_item(ui, Icon::Add, layout.name(), None) {
                                self.run(Command::AddLayoutSlide { at: self.slide + 1, layout });
                                self.session.seal();
                                self.select_slide(self.slide + 1);
                            }
                        }
                    });
                });
            });
        }));

        let items = self.project().slides[self.slide].elements.len();
        let rows_height = RULER + SLIDE_ROW + ROW * items.max(1) as f32 + MUSIC_ROW + VOICE_ROW + 12.0;
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let (labels, _) = ui.allocate_exact_size(vec2(LABELS, rows_height), Sense::hover());
                self.track_labels(ui, labels);
                egui::ScrollArea::horizontal().id_salt("timeline-h").scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Never, ..Default::default() }).auto_shrink([false, true]).show(ui, |ui| {
                    let width = (total as f32 * zoom + 120.0).max(ui.available_width());
                    let (area, response) = ui.allocate_exact_size(vec2(width, rows_height), Sense::click_and_drag());
                    self.tracks(ui, frame, area, &response, zoom);
                });
            });
        });
        // Release can arrive outside the tracks or while their rows rebuild.
        if ui.input(|i| !i.pointer.primary_down() || !i.focused || i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(false)))) && self.timeline_drag.take().is_some() {
            self.session.seal();
        }
    }

    /// Where the playhead is: playback time, a scrubbed moment, or the
    /// current slide at rest.
    pub(crate) fn playhead(&self) -> Tick {
        if self.playback.playing { self.playback.time } else { self.edit_time() }
    }

    /// The left column: track names, the selected slide's items, and the music
    /// volume and remove controls.
    fn track_labels(&mut self, ui: &mut egui::Ui, r: Rect) {
        let p = palette(ui.ctx());
        // Long names are cut at the column's edge rather than spilling over.
        let painter = &ui.painter().with_clip_rect(r.shrink2(vec2(2.0, 0.0)));
        let label = |y: f32, text: &str, icon: Option<Icon>, strong: bool, color: Color32| {
            let mut x = r.left() + 14.0;
            if let Some(icon) = icon {
                icons::paint(painter, Rect::from_center_size(pos2(x + 7.0, y), Vec2::splat(14.0)), icon, color);
                x += 20.0;
            }
            let font = FontId::new(if strong { 13.0 } else { 12.5 }, if strong { theme::bold() } else { egui::FontFamily::Proportional });
            painter.text(pos2(x, y), Align2::LEFT_CENTER, tr(text), font, color);
        };
        let mut y = r.top() + RULER;
        label(y + SLIDE_ROW / 2.0, "Slides", None, true, p.fg);
        y += SLIDE_ROW;
        let slide = &self.project().slides[self.slide];
        if slide.elements.is_empty() {
            label(y + ROW / 2.0, "No items", None, false, p.dim);
            y += ROW;
        }
        let mut clicked_item = None;
        // Front-most first, like a layers list.
        for i in (0..slide.elements.len()).rev() {
            let (icon, name) = item_name(&slide.elements[i].kind);
            let selected = self.selected == Some(i);
            if selected {
                painter.rect_filled(Rect::from_min_size(pos2(r.left() + 4.0, y + 2.0), vec2(LABELS - 8.0, ROW - 4.0)), 6, p.accent_bg.gamma_multiply(0.18));
            }
            let response = ui.interact(Rect::from_min_size(pos2(r.left() + 4.0, y), vec2(LABELS - 8.0, ROW)), ui.scope_id().with(("track-item", self.generation, self.slide, i)), Sense::click());
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, selected, &name));
            if response.clicked() { clicked_item = Some(i); }
            label(y + ROW / 2.0, &name, Some(icon), false, if selected { p.accent } else { p.fg });
            y += ROW;
        }
        if let Some(i) = clicked_item { self.selected = Some(i); }
        label(y + 17.0, "Music", Some(Icon::Video), true, p.fg);
        if let Some(music) = self.project().music.clone() {
            {
                let mut controls = ui.new_child(egui::UiBuilder::new().id_salt("music-remove").max_rect(Rect::from_min_size(pos2(r.right() - 40.0, y), vec2(34.0, 34.0))));
                if adw::icon_button(&mut controls, Icon::Delete, "Remove music").clicked() { self.remove_timeline_music(); }
            }
            {
                let mut controls = ui.new_child(egui::UiBuilder::new().id_salt("music-volume").max_rect(Rect::from_min_size(pos2(r.left() + 12.0, y + 34.0), vec2(LABELS - 24.0, 30.0))));
                let ui = &mut controls;
                let mut volume = (f64::from(music.volume) * 100.0).round();
                // The same slider as the inspector's, with the level beside it.
                let response = ui.horizontal(|ui| {
                    let response = ui.allocate_ui(vec2(LABELS - 76.0, 30.0), |ui| adw::slider(ui, &mut volume, 0.0..=100.0, 1.0, "Timeline music volume")).inner;
                    ui.label(egui::RichText::new(format!("{volume:.0}%")).size(12.5).color(p.dim));
                    response
                }).inner;
                if response.changed() && self.project().music.is_some() {
                    self.stop_playback();
                    self.update_music(|m| m.volume = (volume / 100.0) as f32);
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) { self.session.seal(); }
            }
        } else {
            label(y + 48.0, "No soundtrack", None, false, p.dim);
        }
        y += MUSIC_ROW;
        label(y + VOICE_ROW / 2.0, "Voice", Some(Icon::Microphone), true, p.fg);
        painter.vline(r.right() - 0.5, r.y_range(), Stroke::new(1.0, p.border));
    }

    /// The scrollable tracks: ruler, slide blocks, item bars, music and voice,
    /// then the playhead; also handles drags and scrubbing.
    fn tracks(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, area: Rect, response: &egui::Response, zoom: f32) {
        let drawn_slide = self.slide;
        let revision = self.session.revision();
        let p = palette(ui.ctx());
        let origin = area.left() + 12.0;
        let x_of = |t: f64| origin + t as f32 * zoom;
        let t_of = |x: f32| f64::from((x - origin) / zoom).max(0.0);
        let total = self.project().total_duration().as_secs_f64();
        let ruler = Rect::from_min_size(area.min, vec2(area.width(), RULER));
        let slides_row = Rect::from_min_size(pos2(area.left(), ruler.bottom()), vec2(area.width(), SLIDE_ROW));
        let items_top = slides_row.bottom();
        let elements = self.project().slides[self.slide].elements.len();
        let sound_row = Rect::from_min_size(pos2(area.left(), items_top + ROW * elements.max(1) as f32), vec2(area.width(), MUSIC_ROW + VOICE_ROW));
        let music_row = Rect::from_min_size(sound_row.min, vec2(area.width(), MUSIC_ROW));
        let voice_row = Rect::from_min_max(pos2(area.left(), music_row.bottom()), sound_row.max);

        self.ruler(ui, ruler, origin, zoom, total);
        ui.interact(ruler, ui.scope_id().with("ruler-accessibility"), Sense::hover()).widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Timeline ruler"));

        // Row backgrounds and lines.
        let painter = ui.painter().clone();
        for y in [slides_row.bottom(), sound_row.top()] {
            painter.hline(area.x_range(), y, Stroke::new(1.0, p.border));
        }

        // Slide blocks.
        let pointer = ui.ctx().pointer_interact_pos();
        let mut start = 0.0;
        let mut blocks = Vec::new();
        for i in 0..self.project().slides.len() {
            let d = self.project().slides[i].duration.as_secs_f64();
            let r = Rect::from_min_max(pos2(x_of(start) + 1.0, slides_row.top() + 6.0), pos2(x_of(start + d) - 1.0, slides_row.bottom() - 6.0));
            blocks.push((i, start, r));
            start += d;
        }
        // Block names are painted after the playhead so it never hides them.
        let mut names = Vec::new();
        for &(i, _, r) in &blocks {
            names.extend(self.slide_block(ui, frame, i, r));
            // A click can select a slide with a different number of items;
            // its menu can also insert or remove slides. Rebuild the rows
            // and blocks before using any of the old geometry or indices.
            if self.slide != drawn_slide || self.session.revision() != revision {
                ui.ctx().request_discard("timeline slide changed");
                ui.ctx().request_repaint();
                return;
            }
        }

        // Item bars for the current slide, front-most on top.
        let slide_start = blocks[self.slide].1;
        let slide = self.project().slides[self.slide].clone();
        let slide_end = slide_start + slide.duration.as_secs_f64();
        let item_rows: Vec<(usize, Rect)> = (0..elements)
            .rev()
            .enumerate()
            .map(|(row, i)| (i, Rect::from_min_size(pos2(area.left(), items_top + ROW * row as f32), vec2(area.width(), ROW))))
            .collect();
        // Shade the span of the current slide across the item rows.
        let span = Rect::from_min_max(pos2(x_of(slide_start), items_top), pos2(x_of(slide_end), sound_row.top()));
        painter.rect_filled(span, 0, p.accent_bg.gamma_multiply(0.06));
        for &(i, row) in &item_rows {
            let e = &slide.elements[i];
            let begin = slide_start + slide.element_start(i);
            let bar = Rect::from_min_size(pos2(x_of(begin), row.top() + 5.0), vec2((slide.element_motion_secs(i) as f32 * zoom).max(10.0), ROW - 10.0));
            let selected = self.selected == Some(i);
            // A thin line shows the item staying on screen until the slide ends.
            painter.hline(bar.right()..=x_of(slide_end) - 2.0, row.center().y, Stroke::new(2.0, p.accent_bg.gamma_multiply(if selected { 0.6 } else { 0.3 })));
            let fill = if e.motion == Motion::None { p.dim } else { p.accent_bg };
            painter.rect_filled(bar, 5, if selected { fill } else { fill.gamma_multiply(0.75) });
            if selected {
                painter.rect_stroke(bar.expand(1.5), 6, Stroke::new(1.5, p.fg.gamma_multiply(0.6)), egui::StrokeKind::Outside);
            }
            if e.start_ms.is_some() {
                // Pinned start: a small marker, double-click to release it.
                painter.circle_filled(pos2(bar.left() + 5.0, bar.center().y), 2.5, Color32::WHITE);
            }
            let bar_response = ui.interact(bar.expand2(vec2(2.0, 0.0)), ui.scope_id().with(("item-bar", self.generation, self.slide, i)), Sense::click_and_drag());
            let bar_response = bar_response.on_hover_text(tr(if e.start_ms.is_some() {
                "Drag to change when it appears. Double-click to go back to automatic."
            } else {
                "Drag to choose when it appears"
            }));
            if bar_response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
            if bar_response.clicked() {
                self.selected = Some(i);
            }
            if bar_response.double_clicked() {
                let mut changed = e.clone();
                changed.start_ms = None;
                self.run(Command::UpdateElement { slide: self.slide, index: i, element: changed });
                self.session.seal();
            }
            if bar_response.drag_started()
                && let Some(pos) = pointer
            {
                self.selected = Some(i);
                self.timeline_drag = Some(TimelineDrag::ItemStart { index: i, origin_ms: (slide.element_start(i) * 1000.0) as i64, grab_x: pos.x });
            }
        }

        let boundaries: Vec<i64> = blocks.iter().map(|(_, s, _)| (s * 1000.0).round() as i64).chain([(total * 1000.0).round() as i64]).collect();
        self.music_track(ui, music_row, &x_of, pointer);
        painter.hline(area.x_range(), music_row.bottom(), Stroke::new(1.0, p.border.gamma_multiply(0.5)));
        self.voice_track(ui, voice_row, &blocks, &x_of);

        // Ongoing drags.
        let released = ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
        if let (Some(drag), Some(pos)) = (self.timeline_drag, pointer) {
            match drag {
                TimelineDrag::Scrub => self.scrub_to(t_of(pos.x)),
                TimelineDrag::Resize { slide, start_x } => {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                    let ms = (f64::from((pos.x - start_x) / zoom) * 1000.0 / 100.0).round() as i64 * 100;
                    let ms = ms.clamp(MIN_SLIDE_MS, MAX_SLIDE_MS);
                    if Tick::from_millis(ms) != self.project().slides[slide].duration {
                        self.run(Command::SetDuration { index: slide, millis: ms });
                    }
                }
                TimelineDrag::Move { slide } => {
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                    let target = drop_index(&blocks, pos.x);
                    let marker_x = if target > slide { blocks[target].2.right() + 1.0 } else { blocks[target].2.left() - 1.0 };
                    painter.vline(marker_x, slides_row.y_range(), Stroke::new(3.0, p.accent_bg));
                }
                TimelineDrag::ItemStart { index, origin_ms, grab_x } => {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                    let max = (slide.duration.as_secs_f64() - slide.elements[index].motion_secs()).max(0.0);
                    let secs = (origin_ms as f64 / 1000.0 + f64::from((pos.x - grab_x) / zoom)).clamp(0.0, max);
                    let ms = (secs * 20.0).round() as i64 * 50;
                    if slide.elements[index].start_ms != Some(ms) {
                        let mut changed = slide.elements[index].clone();
                        changed.start_ms = Some(ms);
                        self.run(Command::UpdateElement { slide: self.slide, index, element: changed });
                    }
                    painter.text(pos2(pos.x, items_top - 2.0), Align2::CENTER_BOTTOM, format!("{:.2} s", ms as f64 / 1000.0), FontId::proportional(11.5), p.fg);
                }
                TimelineDrag::MusicMove { grab_x, start_ms, end_ms, until_end } => {
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                    let total_ms = boundaries.last().copied().unwrap_or(0);
                    let length = end_ms - start_ms;
                    let wanted = start_ms + (f64::from((pos.x - grab_x) / zoom) * 1000.0) as i64;
                    let wanted = if ui.input(|i| i.modifiers.shift) {
                        let by_start = snap(wanted, &boundaries, zoom);
                        let by_end = snap(wanted + length, &boundaries, zoom) - length;
                        if by_start != wanted { by_start } else { by_end }
                    } else { wanted };
                    let total = self.project().total_duration();
                    self.update_music(|m| {
                        // Always calculate from the drag's original span, so dragging
                        // back from the right edge restores the full clip length.
                        m.start_ms = start_ms;
                        m.end_ms = if until_end { None } else { Some(end_ms) };
                        m.move_to(wanted, total);
                    });
                    let start = wanted.clamp(0, (total_ms - kn_model::MIN_MUSIC_MS).max(0));
                    painter.text(pos2(pos.x, music_row.top() - 2.0), Align2::CENTER_BOTTOM, crate::trf!("starts at {time}", time = clock(start as f64 / 1000.0)), FontId::proportional(11.5), p.fg);
                }
                TimelineDrag::MusicStart { grab_x, start_ms, offset_ms } => {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                    let end = self.project().music.as_ref().map(|m| m.span(self.project().total_duration()).1).unwrap_or_default();
                    let end_ms = (end.as_secs_f64() * 1000.0) as i64;
                    let wanted = start_ms + (f64::from((pos.x - grab_x) / zoom) * 1000.0) as i64;
                    let wanted = if ui.input(|i| i.modifiers.shift) { snap(wanted, &boundaries, zoom) } else { wanted };
                    // The song can't start before its own beginning.
                    let start = wanted.clamp(start_ms - offset_ms, end_ms - kn_model::MIN_MUSIC_MS).max(0);
                    self.update_music(|m| {
                        m.start_ms = start;
                        m.offset_ms = (offset_ms + start - start_ms).max(0);
                    });
                    painter.text(pos2(pos.x, music_row.top() - 2.0), Align2::CENTER_BOTTOM, crate::trf!("song from {time}", time = clock((offset_ms + start - start_ms) as f64 / 1000.0)), FontId::proportional(11.5), p.fg);
                }
                TimelineDrag::MusicEnd { grab_x, end_ms } => {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                    let total_ms = boundaries.last().copied().unwrap_or(0);
                    let start_ms = self.project().music.as_ref().map_or(0, |m| (m.span(self.project().total_duration()).0.as_secs_f64() * 1000.0) as i64);
                    let wanted = end_ms + (f64::from((pos.x - grab_x) / zoom) * 1000.0) as i64;
                    let wanted = if ui.input(|i| i.modifiers.shift) { snap(wanted, &boundaries, zoom) } else { wanted };
                    let end = wanted.clamp(start_ms + kn_model::MIN_MUSIC_MS, total_ms);
                    self.update_music(|m| m.end_ms = if end >= total_ms { None } else { Some(end) });
                    let label = if end >= total_ms { tr("until the end").to_owned() } else { crate::trf!("ends at {time}", time = clock(end as f64 / 1000.0)) };
                    painter.text(pos2(pos.x, music_row.top() - 2.0), Align2::CENTER_BOTTOM, label, FontId::proportional(11.5), p.fg);
                }
            }
            if released {
                if let TimelineDrag::Move { slide } = drag {
                    let target = drop_index(&blocks, pos.x);
                    if target != slide {
                        self.run(Command::MoveSlide { from: slide, to: target });
                        self.select_slide(target);
                    }
                }
                self.timeline_drag = None;
                self.session.seal();
            }
        } else if released {
            self.timeline_drag = None;
        }

        // Clicking or dragging on the ruler (or empty space) scrubs.
        if (response.drag_started() || response.clicked())
            && self.timeline_drag.is_none()
            && let Some(pos) = response.interact_pointer_pos()
            && ruler.contains(pos)
        {
            if response.drag_started() && ui.input(|i| i.pointer.primary_down()) {
                self.timeline_drag = Some(TimelineDrag::Scrub);
            }
            self.scrub_to(t_of(pos.x));
        }

        // The playhead, over everything.
        let x = x_of(self.playhead().as_secs_f64());
        painter.vline(x, area.top()..=sound_row.bottom(), Stroke::new(2.0, p.destructive_bg));
        painter.add(egui::Shape::convex_polygon(vec![pos2(x - 6.0, area.top()), pos2(x + 6.0, area.top()), pos2(x, area.top() + 8.0)], p.destructive_bg, Stroke::NONE));
        for &(i, at) in &names {
            self.paint_block_name(ui, i, at);
        }
        let visible = ui.clip_rect().intersect(area);
        if self.playback.playing && self.timeline_drag.is_none() && !ui.input(|i| i.pointer.primary_down())
            && let Some(left) = follow_page(x, visible.left(), visible.right()) {
                ui.scroll_to_rect_animation(
                    Rect::from_min_size(pos2(left, ruler.top()), vec2(1.0, 1.0)),
                    Some(egui::Align::Min), egui::style::ScrollAnimation::none(),
                );
        }
    }

    /// Second marks along the top, with labels spaced to stay readable.
    fn ruler(&self, ui: &egui::Ui, r: Rect, origin: f32, zoom: f32, total: f64) {
        let p = palette(ui.ctx());
        let painter = ui.painter();
        painter.rect_filled(r, 0, p.sidebar);
        // Label every 1, 2, 5 or 10 seconds, whatever keeps labels apart.
        let step = [1.0, 2.0, 5.0, 10.0, 30.0].into_iter().find(|s| s * zoom >= 48.0).unwrap_or(60.0);
        let mut t = 0.0f32;
        while t <= total as f32 + step {
            let x = origin + t * zoom;
            painter.vline(x, (r.bottom() - 8.0)..=r.bottom(), Stroke::new(1.0, p.dim));
            painter.text(pos2(x + 3.0, r.top() + 4.0), Align2::LEFT_TOP, format!("{}:{:02}", t as u64 / 60, t as u64 % 60), FontId::proportional(11.0), p.dim);
            for k in 1..(step as i32).min(10) {
                let xx = x + k as f32 * (step / step.min(10.0)) * zoom;
                painter.vline(xx, (r.bottom() - 4.0)..=r.bottom(), Stroke::new(1.0, p.border));
            }
            t += step;
        }
        painter.hline(r.x_range(), r.bottom(), Stroke::new(1.0, p.border));
        if ui.rect_contains_pointer(r) {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
    }

    /// Draws slide `i`'s block; returns where its number and length go.
    fn slide_block(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, i: usize, r: Rect) -> Option<(usize, egui::Pos2)> {
        let p = palette(ui.ctx());
        let selected = i == self.slide;
        let id = ui.scope_id().with(("slide-block", self.generation, i));
        let body = ui.interact(r, id, Sense::click_and_drag());
        let edge = ui.interact(Rect::from_min_max(pos2(r.right() - EDGE, r.top()), r.max), id.with("edge"), Sense::drag());
        body.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, selected, format!("Slide {}", i + 1)));

        let mut name = None;
        let painter = ui.painter();
        painter.rect(r, 8, p.card, Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { p.accent_bg } else { p.border }), egui::StrokeKind::Inside);
        // Thumbnail at the start of the block, if it fits.
        let thumb_h = r.height() - 10.0;
        let (cw, ch) = self.project().aspect.canvas_size();
        let thumb_w = (thumb_h * (cw / ch) as f32).min(r.width() - 10.0);
        if thumb_w > 24.0 {
            let thumb = Rect::from_min_size(r.min + vec2(5.0, 5.0), vec2(thumb_w, thumb_h));
            let slide = &self.project().slides[i];
            let time = self.slide_start(i) + slide.rest_time();
            let ppp = ui.ctx().pixels_per_point();
            let px = [(thumb.width() * ppp).round() as u32, (thumb.height() * ppp).round() as u32];
            let stamp = Some(self.content_stamp());
            if ui.is_rect_visible(thumb) && let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) {
                let (project, media) = (self.session.project(), &self.session.media);
                let tex = canvas.paint(rs, Surface::Thumbnail(i), Frame { project, media, time, size: px, stamp }, &mut self.render);
                egui::Image::new((tex, thumb.size())).corner_radius(5).paint_at(ui, thumb);
            }
            let text_x = thumb.right() + 8.0;
            if r.right() - text_x > 30.0 {
                name = Some((i, pos2(text_x, r.top() + 8.0)));
            }
        }
        // How the slide arrives, at its left edge.
        let transition = self.project().slides[i].transition;
        if i > 0 && transition != Transition::Cut {
            let c = pos2(r.left() - 1.0, r.center().y);
            ui.painter().add(egui::Shape::convex_polygon(vec![c + vec2(0.0, -6.0), c + vec2(6.0, 0.0), c + vec2(0.0, 6.0), c + vec2(-6.0, 0.0)], p.accent_bg, Stroke::new(1.5, p.card)));
        }
        // The length handle.
        if edge.hovered() || edge.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            ui.painter().rect_filled(Rect::from_min_max(pos2(r.right() - 4.0, r.top() + 10.0), pos2(r.right() - 1.0, r.bottom() - 10.0)), 2, p.accent_bg);
        }

        if edge.drag_started() {
            self.timeline_drag = Some(TimelineDrag::Resize { slide: i, start_x: r.left() - 1.0 });
            self.select_slide(i);
        } else if body.drag_started() {
            self.timeline_drag = Some(TimelineDrag::Move { slide: i });
            self.select_slide(i);
        } else if body.clicked() {
            self.select_slide(i);
        }
        let hint = crate::trf!("Slide {n}: drag to move, drag its right edge to change its length, right-click for more", n = i + 1);
        let body = body.on_hover_text(hint);
        egui::Popup::context_menu(&body).width(180.0).show(|ui| self.slide_menu(ui, i));
        name
    }

    /// A slide block's number and length on a small backing in the block's
    /// colour, drawn over the playhead.
    fn paint_block_name(&self, ui: &egui::Ui, i: usize, at: egui::Pos2) {
        let Some(slide) = self.project().slides.get(i) else { return };
        let p = palette(ui.ctx());
        let painter = ui.painter();
        let number = painter.layout_no_wrap(format!("{}", i + 1), FontId::new(13.0, theme::bold()), if i == self.slide { p.accent } else { p.fg });
        let length = painter.layout_no_wrap(format!("{:.1} s", slide.duration.as_secs_f64()), FontId::proportional(11.5), p.dim);
        let width = number.size().x.max(length.size().x);
        painter.rect_filled(Rect::from_min_size(at - vec2(3.0, 2.0), vec2(width + 6.0, 36.0)), 4, p.card);
        painter.galley(at, number, p.fg);
        painter.galley(at + vec2(0.0, 18.0), length, p.dim);
    }

    /// Changes the music (one undo step per drag, sealed on release).
    fn update_music(&mut self, change: impl FnOnce(&mut kn_model::Music)) {
        let Some(mut music) = self.project().music.clone() else { return };
        let before = music.clone();
        change(&mut music);
        if music != before {
            self.run(Command::SetMusic { music: Some(music) });
        }
    }

    /// The music as a clip: drag it to move, its edges to trim.
    fn music_track(&mut self, ui: &mut egui::Ui, row: Rect, x_of: &dyn Fn(f64) -> f32, pointer: Option<egui::Pos2>) {
        let p = palette(ui.ctx());
        let Some(music) = self.project().music.clone() else {
            ui.painter().text(pos2(x_of(0.0) + 4.0, row.center().y), Align2::LEFT_CENTER, tr("No music yet: choose some under Sound in the panel on the right"), FontId::proportional(11.5), p.dim);
            return;
        };
        let (start, end) = music.span(self.project().total_duration());
        let (start_s, end_s) = (start.as_secs_f64(), end.as_secs_f64());
        let bar = Rect::from_min_max(pos2(x_of(start_s), row.top() + 4.0), pos2(x_of(end_s), row.bottom() - 4.0));
        let green = Color32::from_rgb(0x26, 0xa2, 0x69);
        let painter = ui.painter().clone();
        painter.rect_filled(bar, 6, green.gamma_multiply(0.35));

        let name = match &music.source {
            MusicSource::Mood(m) => crate::trf!("{mood} music", mood = crate::i18n::tr(m.name())),
            MusicSource::File(_) => tr("Your song").into(),
        };
        if ui.is_rect_visible(bar) {
            let needed = music.offset_ms.max(0) as f64 / 1000.0 + end_s - start_s;
            let view = self.waveform(&music.source, needed, ui.ctx());
            if let WaveView::Ready(waveform) = view {
                let wave = Rect::from_min_max(bar.min + vec2(2.0, 23.0), bar.max - vec2(2.0, 4.0));
                let zoom = x_of(1.0) - x_of(0.0);
                let span = WaveSpan { offset: music.offset_ms.max(0) as f64 / 1000.0, zoom: f64::from(zoom), looped: music.looped, gain: music.volume };
                paint_waveform(ui, wave, &waveform, &span, green);
            } else {
                let status = tr(if matches!(view, WaveView::Loading) { "Loading waveform…" } else { "Audio unavailable" });
                painter.text(bar.left_bottom() + vec2(8.0, -8.0), Align2::LEFT_BOTTOM, status, FontId::proportional(10.5), p.dim);
            }
        }
        // The name sits on a small pill so it reads over the waveform.
        let volume = format!("{:.0}%", music.volume * 100.0);
        let label = if music.offset_ms > 0 { crate::i18n::row(&[&name, &volume, &crate::trf!("from {time}", time = clock(music.offset_ms as f64 / 1000.0))], " · ") } else { crate::i18n::row(&[&name, &volume], " · ") };
        let galley = painter.layout_no_wrap(label, FontId::new(11.0, theme::semibold()), p.fg);
        let pill = Rect::from_min_size(bar.left_top() + vec2(6.0, 4.0), galley.size() + vec2(10.0, 2.0));
        if pill.right() < bar.right() - 4.0 {
            painter.rect_filled(pill, 6, p.card.gamma_multiply(0.85));
            painter.galley(pill.min + vec2(5.0, 1.0), galley, p.fg);
        }

        // Edges trim, the body moves.
        let id = ui.scope_id().with(("music", self.generation));
        let left = ui.interact(Rect::from_min_max(bar.min, pos2(bar.left() + EDGE, bar.bottom())), id.with("left"), Sense::drag());
        let right = ui.interact(Rect::from_min_max(pos2(bar.right() - EDGE, bar.top()), bar.max), id.with("right"), Sense::drag());
        let body = ui.interact(bar.shrink2(vec2(EDGE, 0.0)), id.with("body"), Sense::click_and_drag());
        for handle in [&left, &right] {
            if handle.hovered() || handle.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                let r = handle.rect;
                painter.rect_filled(Rect::from_center_size(r.center(), vec2(3.0, r.height() - 10.0)), 2, green);
            }
        }
        if body.hovered() && !body.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        body.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Move music clip"));
        let body = body.on_hover_text(tr("Drag to move; drag edges to trim. Hold Shift to snap. Right-click for music actions."));
        egui::Popup::context_menu(&body).show(|ui| {
            if crate::header::menu_item(ui, Icon::Delete, "Remove music", None) { self.remove_timeline_music(); }
        });
        if self.project().music.is_none() { return; }
        let ms = |t: Tick| (t.as_secs_f64() * 1000.0).round() as i64;
        if let Some(pos) = pointer {
            if left.drag_started() {
                self.timeline_drag = Some(TimelineDrag::MusicStart { grab_x: ui.input(|i| i.pointer.press_origin()).unwrap_or(pos).x, start_ms: ms(start), offset_ms: music.offset_ms });
            } else if right.drag_started() {
                self.timeline_drag = Some(TimelineDrag::MusicEnd { grab_x: ui.input(|i| i.pointer.press_origin()).unwrap_or(pos).x, end_ms: ms(end) });
            } else if body.drag_started() {
                self.timeline_drag = Some(TimelineDrag::MusicMove { grab_x: ui.input(|i| i.pointer.press_origin()).unwrap_or(pos).x, start_ms: ms(start), end_ms: ms(end), until_end: music.end_ms.is_none() });
            }
        }
    }

    /// Removes the music as one undo step, with an Undo toast.
    fn remove_timeline_music(&mut self) {
        self.stop_playback();
        self.timeline_drag = None;
        self.session.seal();
        self.run(Command::SetMusic { music: None });
        self.session.seal();
        self.notify_undo("Music removed");
    }

    /// File peaks are cached by media ID; generated tracks are analysed once
    /// per duration bucket on a worker so a long project stays responsive.
    fn waveform(&mut self, source: &MusicSource, needed: f64, ctx: &egui::Context) -> WaveView {
        let seconds = waveform_seconds(needed);
        let key = waveform_key(source, needed);
        if !self.waveform.contains_key(&key) {
            let waveform = match source {
                MusicSource::File(id) => {
                    match self.audio_cache.get(&self.session.media, id) {
                        Some(pcm) => Waveform::Ready(std::sync::Arc::new(WaveformData { peaks: kn_audio::peaks(&pcm, PEAKS_PER_SECOND), seconds: pcm.seconds() })),
                        None => Waveform::Unavailable,
                    }
                }
                MusicSource::Mood(mood) => {
                    let mood = *mood;
                    let (tx, rx) = std::sync::mpsc::channel();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let pcm = kn_audio::generate(mood, seconds as usize * kn_audio::RATE as usize);
                        let _ = tx.send(WaveformData { peaks: kn_audio::peaks(&pcm, PEAKS_PER_SECOND), seconds: pcm.seconds() });
                        ctx.request_repaint();
                    });
                    Waveform::Loading(rx)
                }
            };
            self.waveform.insert(key.clone(), waveform);
        }
        let Some(waveform) = self.waveform.get_mut(&key) else { return WaveView::Unavailable };
        if let Waveform::Loading(rx) = waveform {
            match rx.try_recv() {
                Ok(data) => *waveform = Waveform::Ready(std::sync::Arc::new(data)),
                Err(std::sync::mpsc::TryRecvError::Empty) => {},
                // The worker stopped without an answer: it won't come.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => *waveform = Waveform::Unavailable,
            }
        }
        match waveform {
            Waveform::Ready(data) => WaveView::Ready(data.clone()),
            Waveform::Loading(_) => WaveView::Loading,
            Waveform::Unavailable => WaveView::Unavailable,
        }
    }

    /// Each slide's recorded voice as a clip with its waveform.
    fn voice_track(&mut self, ui: &egui::Ui, row: Rect, blocks: &[(usize, f64, Rect)], x_of: &dyn Fn(f64) -> f32) {
        let p = palette(ui.ctx());
        let painter = ui.painter();
        let orange = Color32::from_rgb(0xe6, 0x61, 0x00);
        let mut any = false;
        for &(i, start, _) in blocks {
            let Some(id) = self.project().slides[i].voice.clone() else { continue };
            any = true;
            let secs = self.audio_cache.get(&self.session.media, &id).map_or(1.0, |pcm| pcm.seconds());
            let begin = start + 0.25;
            let end = (begin + secs).min(self.project().total_duration().as_secs_f64());
            let bar = Rect::from_min_max(pos2(x_of(begin), row.top() + 4.0), pos2(x_of(end), row.bottom() - 4.0));
            painter.rect_filled(bar, 5, orange.gamma_multiply(0.18));
            if ui.is_rect_visible(bar)
                && let WaveView::Ready(waveform) = self.waveform(&MusicSource::File(id), secs, ui.ctx()) {
                    let wave = Rect::from_min_max(bar.min + vec2(2.0, 20.0), bar.max - vec2(2.0, 3.0));
                    let span = WaveSpan { offset: 0.0, zoom: f64::from(x_of(1.0) - x_of(0.0)), looped: false, gain: 1.0 };
                    paint_waveform(ui, wave, &waveform, &span, orange);
            }
            let painter = painter.with_clip_rect(bar.intersect(ui.clip_rect()));
            painter.text(bar.left_top() + vec2(7.0, 4.0), Align2::LEFT_TOP, crate::trf!("Voice {n}", n = i + 1), FontId::new(11.0, theme::semibold()), p.fg);
        }
        if !any {
            painter.text(pos2(x_of(0.0) + 4.0, row.center().y), Align2::LEFT_CENTER, tr("No voice yet: record one for a slide in the panel on the right"), FontId::proportional(11.5), p.dim);
        }
    }

    /// Shows the moment `secs` into the video without playing.
    pub(crate) fn scrub_to(&mut self, secs: f64) {
        self.stop_playback();
        let total = self.project().total_duration();
        let t = Tick((secs * kn_time::TICKS_PER_SECOND as f64) as i64).min(total - Tick(1)).max(Tick::ZERO);
        if let Some((index, _)) = self.project().slide_at(t) {
            if index != self.slide {
                self.selected = None;
            }
            self.slide = index;
        }
        self.scrub = Some(t);
    }
}

/// Relative source amplitude. Keep source time aligned when clips move,
/// trim, or loop, and retain short peaks when the timeline is zoomed out.
fn waveform_peak(data: &WaveformData, source: f64, window: f64, looped: bool) -> f32 {
    if data.peaks.is_empty() || data.seconds <= 0.0 || source < 0.0 || (!looped && source >= data.seconds) { return 0.0; }
    let source = if looped { source.rem_euclid(data.seconds) } else { source };
    let first = (source * PEAKS_PER_SECOND as f64 + 1e-9).floor() as usize;
    let count = (window * PEAKS_PER_SECOND as f64).ceil().max(1.0) as usize;
    (first..first + count.min(data.peaks.len())).filter_map(|i| {
        if looped { data.peaks.get(i % data.peaks.len()) } else { data.peaks.get(i) }
    }).copied().fold(0.0_f32, f32::max)
}

/// What [`KinettoApp::waveform`] has for a sound right now.
pub(crate) enum WaveView {
    /// Ready to draw.
    Ready(std::sync::Arc<WaveformData>),
    /// Generated music still being analysed.
    Loading,
    /// The sound couldn't be decoded or analysed.
    Unavailable,
}

/// Generated music is analysed in 30-second steps, at least 30 s long.
fn waveform_seconds(needed: f64) -> u64 {
    ((needed.max(0.0) / 30.0).ceil() * 30.0).max(30.0) as u64
}

/// The cache key for a sound's waveform: files by id, generated music by
/// mood and analysed length.
fn waveform_key(source: &MusicSource, needed: f64) -> String {
    match source {
        MusicSource::File(id) => format!("file:{id}"),
        MusicSource::Mood(mood) => format!("mood:{mood:?}:{}", waveform_seconds(needed)),
    }
}

/// Which stretch of a source a clip shows, and how loud it is drawn.
struct WaveSpan {
    /// Source seconds at the clip's left edge.
    offset: f64,
    /// Points per second.
    zoom: f64,
    /// Repeat the sound to fill the clip.
    looped: bool,
    /// Volume, scaling the drawn height.
    gain: f32,
}

/// Draws `data` as vertical peak lines filling `rect`, for the part of the
/// source that `span` shows.
fn paint_waveform(ui: &egui::Ui, rect: Rect, data: &WaveformData, span: &WaveSpan, color: Color32) {
    let WaveSpan { offset, zoom, looped, gain } = *span;
    if rect.height() <= 0.0 || zoom <= 0.0 { return; }
    let visible = rect.intersect(ui.clip_rect());
    let painter = ui.painter().with_clip_rect(visible);
    painter.hline(visible.x_range(), rect.center().y, Stroke::new(1.0, color.gamma_multiply(0.3)));
    let mut x = visible.left();
    while x < visible.right() {
        let source = offset + f64::from(x - rect.left()) / zoom;
        let peak = waveform_peak(data, source, 2.0 / zoom, looped) * gain.clamp(0.0, 1.0);
        let height = peak * rect.height() / 2.0;
        if height > 0.0 { painter.vline(x, (rect.center().y - height)..=(rect.center().y + height), Stroke::new(1.5, color)); }
        x += 2.5;
    }
}

/// The slide position a block dropped at `x` should move to.
fn drop_index(blocks: &[(usize, f64, Rect)], x: f32) -> usize {
    blocks.iter().position(|(_, _, r)| x < r.right()).unwrap_or(blocks.len().saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_and_restart_keep_the_project_unchanged() {
        use egui_kittest::kittest::Queryable;
        let mut h = egui_kittest::Harness::builder().with_size([1280.0, 900.0]).build_eframe(|cc| KinettoApp::new(cc));
        let project = kn_model::Project::sample();
        let app = h.state_mut();
        app.session = kn_engine::Session::new(project.clone());
        app.choosing_template = false;
        app.recovery = None;
        app.restorable = None;
        app.settings.layout = crate::settings::Layout::Timeline;
        app.settings.timeline_zoom = Some(240.0);
        app.scrub = Some(Tick::from_millis(1500));
        h.run_steps(2);
        h.get_by_label("Fit").click();
        h.run_steps(2);
        assert!(h.state().settings.timeline_zoom.unwrap() < 240.0);
        h.get_by_label("Restart video").click();
        h.run_steps(2);
        assert_eq!(h.state().playhead(), Tick::ZERO);
        assert_eq!(h.state().project(), &project);
    }

    #[test]
    fn failed_music_analysis_reads_unavailable_not_loading() {
        let mut app = egui_kittest::Harness::builder().build_eframe(|cc| KinettoApp::new(cc)).into_state();
        let source = MusicSource::Mood(kn_model::Mood::Calm);
        let ctx = egui::Context::default();
        // Still working: loading.
        let (tx, rx) = std::sync::mpsc::channel();
        app.waveform.insert(waveform_key(&source, 10.0), Waveform::Loading(rx));
        assert!(matches!(app.waveform(&source, 10.0, &ctx), WaveView::Loading));
        // The worker stops without sending anything: unavailable, for good.
        drop(tx);
        assert!(matches!(app.waveform(&source, 10.0, &ctx), WaveView::Unavailable));
        assert!(matches!(app.waveform(&source, 10.0, &ctx), WaveView::Unavailable));
    }

    #[test]
    fn dropping_lands_on_the_block_under_the_pointer() {
        let r = |a: f32, b: f32| Rect::from_min_max(pos2(a, 0.0), pos2(b, 10.0));
        let blocks = vec![(0, 0.0, r(0.0, 100.0)), (1, 1.0, r(100.0, 200.0)), (2, 2.0, r(200.0, 300.0))];
        assert_eq!(drop_index(&blocks, 50.0), 0);
        assert_eq!(drop_index(&blocks, 150.0), 1);
        assert_eq!(drop_index(&blocks, 9999.0), 2);
    }

    #[test]
    fn long_text_names_are_shortened() {
        let kind = ElementKind::Text { text: "A very long title that keeps going on".into(), color: kn_geom::Color::rgb(0, 0, 0), size: 10.0, bold: false, align: kn_model::TextAlign::Start, font: kn_model::TextFont::default() };
        let (_, name) = item_name(&kind);
        assert!(name.ends_with('…') && name.chars().count() == 23);
    }

    #[test]
    fn music_snaps_to_nearby_boundaries_only() {
        let targets = [0, 4000, 8000];
        // At 60 points a second, 8 points is about 133 ms.
        assert_eq!(snap(4100, &targets, 60.0), 4000);
        assert_eq!(snap(4300, &targets, 60.0), 4300);
        assert_eq!(snap(-50, &targets, 60.0), 0);
    }

    #[test]
    fn waveform_sampling_handles_trims_loops_and_zoom() {
        let data = WaveformData { peaks: vec![0.0, 0.2, 1.0, 0.4], seconds: 0.2 };
        assert_eq!(waveform_peak(&data, 0.0, 0.01, false), 0.0);
        assert_eq!(waveform_peak(&data, 0.1, 0.01, false), 1.0);
        assert_eq!(waveform_peak(&data, 0.3, 0.01, false), 0.0);
        assert_eq!(waveform_peak(&data, 0.3, 0.01, true), 1.0);
        assert_eq!(waveform_peak(&data, 0.0, 0.2, false), 1.0);
    }

    #[test]
    fn generated_music_waveforms_are_real_audio_peaks() {
        let mut h = egui_kittest::Harness::builder().build_eframe(|cc| KinettoApp::new(cc));
        let ctx = h.ctx.clone();
        let source = MusicSource::Mood(kn_model::Mood::Calm);
        let _ = h.state_mut().waveform(&source, 7.0, &ctx);
        let data = match h.state_mut().waveform.values_mut().next().unwrap() {
            Waveform::Loading(rx) => std::sync::Arc::new(rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap()),
            Waveform::Ready(data) => data.clone(),
            Waveform::Unavailable => panic!("Expected generated audio peaks"),
        };
        assert_eq!(data.seconds, 30.0);
        assert_eq!(data.peaks.len(), 30 * PEAKS_PER_SECOND);
        assert!(data.peaks.iter().all(|p| (0.0..=1.0).contains(p)));
        assert!(data.peaks.iter().copied().fold(0.0_f32, f32::max) > 0.9);
        assert!(data.peaks.windows(2).any(|p| (p[0] - p[1]).abs() > 0.01));
    }

    #[test]
    fn clock_shows_tenths() {
        assert_eq!(clock(65.25), "1:05.2");
    }
}

#[cfg(test)]
mod follow_tests {
    use super::*;
    #[test]
    fn playback_pages_once_then_stays_still() {
        for width in [300.0, 600.0, 1000.0] {
            let left = 100.0;
            let right = left + width;
            assert_eq!(follow_page(right - 20.0, left, right), None);
            let head = right - 15.0;
            let next = follow_page(head, left, right).unwrap();
            assert_eq!(next, head - 24.0);
            for delta in [0.0, 1.0, 10.0, 50.0] {
                assert_eq!(follow_page(head + delta, next, next + width), None);
            }
        }
    }
    #[test]
    fn restart_and_backwards_seek_return_to_the_playhead() {
        assert_eq!(follow_page(12.0, 1200.0, 1800.0), Some(-12.0));
        assert_eq!(follow_page(300.0, 1200.0, 1800.0), Some(276.0));
    }
    #[test]
    fn invalid_or_tiny_viewports_do_not_scroll() {
        assert_eq!(follow_page(100.0, 0.0, 30.0), None);
        assert_eq!(follow_page(f32::NAN, 0.0, 600.0), None);
    }
}

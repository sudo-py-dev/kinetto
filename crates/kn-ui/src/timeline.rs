//! The timeline layout: the whole video laid out in time under the slide.
//! Slides are blocks (drag to reorder, drag the right edge to change their
//! length), the current slide's items are bars (drag to change when they
//! arrive), sound has its own row, and the ruler scrubs.

use crate::KinettoApp;
use crate::adw::{self, Kind};
use crate::canvas::{Frame, Surface};
use crate::icons::{self, Icon};
use crate::theme::{self, palette};
use egui::{Align2, Color32, CursorIcon, FontId, Rect, Sense, Stroke, Vec2, pos2, vec2};
use kn_engine::Command;
use kn_model::{ENTRANCE_SECS, ElementKind, Motion, MusicSource, Transition};
use kn_time::Tick;

pub const DEFAULT_HEIGHT: f32 = 300.0;
const LABELS: f32 = 128.0;
const RULER: f32 = 24.0;
const SLIDE_ROW: f32 = 58.0;
const ROW: f32 = 28.0;
const MUSIC_ROW: f32 = 36.0;
const EDGE: f32 = 7.0;
const DEFAULT_ZOOM: f32 = 60.0;
const MIN_SLIDE_MS: i64 = 1000;
const MAX_SLIDE_MS: i64 = 20_000;

#[derive(Clone, Copy, Debug)]
pub(crate) enum TimelineDrag {
    Scrub,
    /// Changing a slide's length from its right edge.
    Resize { slide: usize, start_x: f32 },
    /// Moving a slide to another place.
    Move { slide: usize },
    /// Changing when an item arrives.
    ItemStart { index: usize, origin_ms: i64, grab_x: f32 },
    /// Moving the music along the video, keeping its length.
    MusicMove { grab_x: f32, start_ms: i64, end_ms: i64 },
    /// Trimming the music's beginning: it starts later in the video and
    /// later in the song, so what plays at each moment stays the same.
    MusicStart { grab_x: f32, start_ms: i64, offset_ms: i64 },
    /// Moving where the music stops.
    MusicEnd { grab_x: f32, end_ms: i64 },
}

/// Waveform peaks per second of song.
const PEAKS_PER_SECOND: usize = 20;
/// Edges and slide boundaries pull the music this close, in points.
const SNAP: f32 = 8.0;

/// `ms`, moved onto the nearest of `targets` when within [`SNAP`] points.
fn snap(ms: i64, targets: &[i64], zoom: f32) -> i64 {
    let reach = (f64::from(SNAP / zoom) * 1000.0) as i64;
    targets.iter().copied().filter(|t| (t - ms).abs() <= reach).min_by_key(|t| (t - ms).abs()).unwrap_or(ms)
}

/// A short, recognisable name for an item.
pub(crate) fn item_name(kind: &ElementKind) -> (Icon, String) {
    match kind {
        ElementKind::Text { text, .. } => {
            let first = text.lines().next().unwrap_or("").trim();
            let short: String = first.chars().take(22).collect();
            let name = if short.is_empty() { "Text".into() } else if first.chars().count() > 22 { format!("{short}…") } else { short };
            (Icon::Text, name)
        }
        ElementKind::Shape { .. } => (Icon::Shape, "Shape".into()),
        ElementKind::Image { .. } => (Icon::Picture, "Picture".into()),
    }
}

fn clock(secs: f64) -> String {
    format!("{}:{:04.1}", (secs / 60.0) as u64, secs % 60.0)
}

impl KinettoApp {
    pub(crate) fn timeline(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let p = palette(ui.ctx());
        let zoom = self.settings.timeline_zoom.unwrap_or(DEFAULT_ZOOM);
        let total = self.project().total_duration().as_secs_f64();

        // Toolbar: play, time, zoom, new slide.
        egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 4 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon, tip) = if self.playing { (Icon::Stop, "Stop  (Space)") } else { (Icon::Play, "Play from this slide  (Space)") };
                if adw::icon_button(ui, icon, tip).clicked() {
                    self.toggle_play();
                }
                let now = self.playhead().as_secs_f64();
                ui.label(egui::RichText::new(format!("{}  /  {}", clock(now), clock(total))).family(theme::semibold()).size(13.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                    ui.add_space(8.0);
                    let mut z = f64::from(zoom);
                    let r = adw::slider(ui, &mut z, 15.0..=240.0, 1.0, "Zoom");
                    if r.changed() {
                        self.settings.timeline_zoom = Some(z as f32);
                    }
                    if r.drag_stopped() {
                        self.settings.save();
                    }
                    ui.label(egui::RichText::new("Zoom").size(12.5).color(p.dim));
                });
            });
        });

        let items = self.project().slides[self.slide].elements.len();
        let rows_height = RULER + SLIDE_ROW + ROW * items.max(1) as f32 + MUSIC_ROW + ROW + 12.0;
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let (labels, _) = ui.allocate_exact_size(vec2(LABELS, rows_height), Sense::hover());
                self.track_labels(ui, labels);
                egui::ScrollArea::horizontal().id_salt("timeline-h").auto_shrink([false, true]).show(ui, |ui| {
                    let width = (total as f32 * zoom + 120.0).max(ui.available_width());
                    let (area, response) = ui.allocate_exact_size(vec2(width, rows_height), Sense::click_and_drag());
                    self.tracks(ui, frame, area, &response, zoom);
                });
            });
        });
    }

    /// Where the playhead is: playback time, a scrubbed moment, or the
    /// current slide at rest.
    fn playhead(&self) -> Tick {
        if self.playing { self.play_time } else { self.edit_time() }
    }

    fn track_labels(&self, ui: &egui::Ui, r: Rect) {
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
            painter.text(pos2(x, y), Align2::LEFT_CENTER, text, font, color);
        };
        let mut y = r.top() + RULER;
        label(y + SLIDE_ROW / 2.0, "Slides", None, true, p.fg);
        y += SLIDE_ROW;
        let slide = &self.project().slides[self.slide];
        if slide.elements.is_empty() {
            label(y + ROW / 2.0, "No items", None, false, p.dim);
            y += ROW;
        }
        // Front-most first, like a layers list.
        for i in (0..slide.elements.len()).rev() {
            let (icon, name) = item_name(&slide.elements[i].kind);
            let selected = self.selected == Some(i);
            if selected {
                painter.rect_filled(Rect::from_min_size(pos2(r.left() + 4.0, y + 2.0), vec2(LABELS - 8.0, ROW - 4.0)), 6, p.accent_bg.gamma_multiply(0.18));
            }
            label(y + ROW / 2.0, &name, Some(icon), false, if selected { p.accent } else { p.fg });
            y += ROW;
        }
        label(y + MUSIC_ROW / 2.0, "Music", Some(Icon::Video), false, p.dim);
        y += MUSIC_ROW;
        label(y + ROW / 2.0, "Voice", Some(Icon::Microphone), false, p.dim);
        painter.vline(r.right() - 0.5, r.y_range(), Stroke::new(1.0, p.border));
    }

    fn tracks(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, area: Rect, response: &egui::Response, zoom: f32) {
        let p = palette(ui.ctx());
        let origin = area.left() + 12.0;
        let x_of = |t: f64| origin + t as f32 * zoom;
        let t_of = |x: f32| f64::from((x - origin) / zoom).max(0.0);
        let total = self.project().total_duration().as_secs_f64();
        let ruler = Rect::from_min_size(area.min, vec2(area.width(), RULER));
        let slides_row = Rect::from_min_size(pos2(area.left(), ruler.bottom()), vec2(area.width(), SLIDE_ROW));
        let items_top = slides_row.bottom();
        let elements = self.project().slides[self.slide].elements.len();
        let sound_row = Rect::from_min_size(pos2(area.left(), items_top + ROW * elements.max(1) as f32), vec2(area.width(), MUSIC_ROW + ROW));
        let music_row = Rect::from_min_size(sound_row.min, vec2(area.width(), MUSIC_ROW));
        let voice_row = Rect::from_min_max(pos2(area.left(), music_row.bottom()), sound_row.max);

        self.ruler(ui, ruler, origin, zoom, total);

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
        for &(i, _, r) in &blocks {
            self.slide_block(ui, frame, i, r);
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
            let bar = Rect::from_min_size(pos2(x_of(begin), row.top() + 5.0), vec2((ENTRANCE_SECS as f32 * zoom).max(10.0), ROW - 10.0));
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
            let bar_response = bar_response.on_hover_text(if e.start_ms.is_some() {
                "Drag to change when it appears. Double-click to go back to automatic."
            } else {
                "Drag to choose when it appears"
            });
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
        let released = ui.input(|i| i.pointer.any_released());
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
                    let max = (slide.duration.as_secs_f64() - ENTRANCE_SECS).max(0.0);
                    let secs = (origin_ms as f64 / 1000.0 + f64::from((pos.x - grab_x) / zoom)).clamp(0.0, max);
                    let ms = (secs * 20.0).round() as i64 * 50;
                    if slide.elements[index].start_ms != Some(ms) {
                        let mut changed = slide.elements[index].clone();
                        changed.start_ms = Some(ms);
                        self.run(Command::UpdateElement { slide: self.slide, index, element: changed });
                    }
                    painter.text(pos2(pos.x, items_top - 2.0), Align2::CENTER_BOTTOM, format!("{:.2} s", ms as f64 / 1000.0), FontId::proportional(11.5), p.fg);
                }
                TimelineDrag::MusicMove { grab_x, start_ms, end_ms } => {
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                    let total_ms = boundaries.last().copied().unwrap_or(0);
                    let length = end_ms - start_ms;
                    let wanted = start_ms + (f64::from((pos.x - grab_x) / zoom) * 1000.0) as i64;
                    // Snap either end of the music to a slide boundary.
                    let by_start = snap(wanted, &boundaries, zoom);
                    let by_end = snap(wanted + length, &boundaries, zoom) - length;
                    let start = if by_start != wanted { by_start } else { by_end }.clamp(0, (total_ms - length).max(0));
                    self.update_music(|m| {
                        m.start_ms = start;
                        m.end_ms = if start + length >= total_ms { None } else { Some(start + length) };
                    });
                    painter.text(pos2(pos.x, music_row.top() - 2.0), Align2::CENTER_BOTTOM, format!("starts at {}", clock(start as f64 / 1000.0)), FontId::proportional(11.5), p.fg);
                }
                TimelineDrag::MusicStart { grab_x, start_ms, offset_ms } => {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                    let end = self.project().music.as_ref().map(|m| m.span(self.project().total_duration()).1).unwrap_or_default();
                    let end_ms = (end.as_secs_f64() * 1000.0) as i64;
                    let wanted = snap(start_ms + (f64::from((pos.x - grab_x) / zoom) * 1000.0) as i64, &boundaries, zoom);
                    // The song can't start before its own beginning.
                    let start = wanted.clamp(start_ms - offset_ms, end_ms - kn_model::MIN_MUSIC_MS).max(0);
                    self.update_music(|m| {
                        m.start_ms = start;
                        m.offset_ms = (offset_ms + start - start_ms).max(0);
                    });
                    painter.text(pos2(pos.x, music_row.top() - 2.0), Align2::CENTER_BOTTOM, format!("song from {}", clock((offset_ms + start - start_ms) as f64 / 1000.0)), FontId::proportional(11.5), p.fg);
                }
                TimelineDrag::MusicEnd { grab_x, end_ms } => {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                    let total_ms = boundaries.last().copied().unwrap_or(0);
                    let start_ms = self.project().music.as_ref().map_or(0, |m| (m.span(self.project().total_duration()).0.as_secs_f64() * 1000.0) as i64);
                    let end = snap(end_ms + (f64::from((pos.x - grab_x) / zoom) * 1000.0) as i64, &boundaries, zoom).clamp(start_ms + kn_model::MIN_MUSIC_MS, total_ms);
                    self.update_music(|m| m.end_ms = if end >= total_ms { None } else { Some(end) });
                    let label = if end >= total_ms { "until the end".to_owned() } else { format!("ends at {}", clock(end as f64 / 1000.0)) };
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
            self.timeline_drag = Some(TimelineDrag::Scrub);
            self.scrub_to(t_of(pos.x));
        }

        // The playhead, over everything.
        let x = x_of(self.playhead().as_secs_f64());
        painter.vline(x, area.top()..=sound_row.bottom(), Stroke::new(2.0, p.destructive_bg));
        painter.add(egui::Shape::convex_polygon(vec![pos2(x - 6.0, area.top()), pos2(x + 6.0, area.top()), pos2(x, area.top() + 8.0)], p.destructive_bg, Stroke::NONE));
        if self.playing {
            ui.scroll_to_rect(Rect::from_center_size(pos2(x, area.center().y), vec2(200.0, 10.0)), None);
        }
    }

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

    fn slide_block(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, i: usize, r: Rect) {
        let p = palette(ui.ctx());
        let selected = i == self.slide;
        let id = ui.scope_id().with(("slide-block", self.generation, i));
        let body = ui.interact(r, id, Sense::click_and_drag());
        let edge = ui.interact(Rect::from_min_max(pos2(r.right() - EDGE, r.top()), r.max), id.with("edge"), Sense::drag());
        body.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, selected, format!("Slide {}", i + 1)));

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
            if let (Some(canvas), Some(rs)) = (self.canvas.as_mut(), frame.wgpu_render_state()) {
                let (project, media) = (self.session.project(), &self.session.media);
                let tex = canvas.paint(rs, Surface::Thumbnail(i), Frame { project, media, time, size: px, stamp }, &mut self.render);
                egui::Image::new((tex, thumb.size())).corner_radius(5).paint_at(ui, thumb);
            }
            let text_x = thumb.right() + 8.0;
            if r.right() - text_x > 30.0 {
                let slide = &self.project().slides[i];
                ui.painter().text(pos2(text_x, r.top() + 8.0), Align2::LEFT_TOP, format!("{}", i + 1), FontId::new(13.0, theme::bold()), if selected { p.accent } else { p.fg });
                ui.painter().text(pos2(text_x, r.top() + 26.0), Align2::LEFT_TOP, format!("{:.1} s", slide.duration.as_secs_f64()), FontId::proportional(11.5), p.dim);
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
        let hint = format!("Slide {}: drag to move, drag its right edge to change its length, right-click for more", i + 1);
        let body = body.on_hover_text(hint);
        egui::Popup::context_menu(&body).width(180.0).show(|ui| self.slide_menu(ui, i));
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
            ui.painter().text(pos2(x_of(0.0) + 4.0, row.center().y), Align2::LEFT_CENTER, "No music yet: choose some under Sound in the panel on the right", FontId::proportional(11.5), p.dim);
            return;
        };
        let (start, end) = music.span(self.project().total_duration());
        let (start_s, end_s) = (start.as_secs_f64(), end.as_secs_f64());
        let bar = Rect::from_min_max(pos2(x_of(start_s), row.top() + 4.0), pos2(x_of(end_s), row.bottom() - 4.0));
        let green = Color32::from_rgb(0x26, 0xa2, 0x69);
        let painter = ui.painter().clone();
        painter.rect_filled(bar, 6, green.gamma_multiply(0.35));

        // Waveform of your own song; generated music just shows its mood.
        let name = match &music.source {
            MusicSource::Mood(m) => format!("{} music", m.name()),
            MusicSource::File(_) => "Your song".into(),
        };
        if let MusicSource::File(id) = &music.source
            && let Some(peaks) = self.waveform(id)
            && !peaks.is_empty()
        {
            let song_secs = peaks.len() as f64 / PEAKS_PER_SECOND as f64;
            let mut x = bar.left() + 1.0;
            while x < bar.right() - 1.0 {
                let mut src = music.offset_ms as f64 / 1000.0 + f64::from(x - bar.left()) / f64::from(x_of(1.0) - x_of(0.0));
                if src >= song_secs {
                    if !music.looped {
                        break;
                    }
                    src %= song_secs;
                }
                let peak = peaks[((src * PEAKS_PER_SECOND as f64) as usize).min(peaks.len() - 1)];
                let h = (peak * (bar.height() - 6.0) / 2.0).max(0.5);
                painter.vline(x, (bar.center().y - h)..=(bar.center().y + h), Stroke::new(1.0, green));
                x += 2.0;
            }
        }
        // The name sits on a small pill so it reads over the waveform.
        let label = if music.offset_ms > 0 { format!("{name}  ·  from {}", clock(music.offset_ms as f64 / 1000.0)) } else { name };
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
        let body = body.on_hover_text("Drag to move the music. Drag its edges to trim it.");
        let ms = |t: Tick| (t.as_secs_f64() * 1000.0).round() as i64;
        if let Some(pos) = pointer {
            if left.drag_started() {
                self.timeline_drag = Some(TimelineDrag::MusicStart { grab_x: pos.x, start_ms: ms(start), offset_ms: music.offset_ms });
            } else if right.drag_started() {
                self.timeline_drag = Some(TimelineDrag::MusicEnd { grab_x: pos.x, end_ms: ms(end) });
            } else if body.drag_started() {
                self.timeline_drag = Some(TimelineDrag::MusicMove { grab_x: pos.x, start_ms: ms(start), end_ms: ms(end) });
            }
        }
    }

    /// Peaks of a song, worked out once per song.
    fn waveform(&mut self, id: &str) -> Option<std::sync::Arc<Vec<f32>>> {
        if self.waveform.as_ref().is_none_or(|(cached, _)| cached != id) {
            let pcm = self.audio_cache.get(&self.session.media, id)?;
            self.waveform = Some((id.to_owned(), std::sync::Arc::new(kn_audio::peaks(&pcm, PEAKS_PER_SECOND))));
        }
        self.waveform.as_ref().map(|(_, p)| p.clone())
    }

    fn voice_track(&mut self, ui: &egui::Ui, row: Rect, blocks: &[(usize, f64, Rect)], x_of: &dyn Fn(f64) -> f32) {
        let p = palette(ui.ctx());
        let painter = ui.painter();
        let mut any = false;
        for &(i, start, _) in blocks {
            let Some(id) = self.project().slides[i].voice.clone() else { continue };
            any = true;
            let secs = self.audio_cache.get(&self.session.media, &id).map_or(1.0, |pcm| pcm.seconds());
            let begin = start + 0.25;
            let bar = Rect::from_min_max(pos2(x_of(begin), row.top() + 4.0), pos2(x_of(begin + secs), row.bottom() - 4.0));
            painter.rect_filled(bar, 5, Color32::from_rgb(0xe6, 0x61, 0x00).gamma_multiply(0.7));
            painter.text(bar.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, format!("Voice {}", i + 1), FontId::proportional(10.5), p.fg);
        }
        if !any {
            painter.text(pos2(x_of(0.0) + 4.0, row.center().y), Align2::LEFT_CENTER, "No voice yet: record one for a slide in the panel on the right", FontId::proportional(11.5), p.dim);
        }
    }

    /// Shows the moment `secs` into the video without playing.
    fn scrub_to(&mut self, secs: f64) {
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

/// The slide position a block dropped at `x` should move to.
fn drop_index(blocks: &[(usize, f64, Rect)], x: f32) -> usize {
    blocks.iter().position(|(_, _, r)| x < r.right()).unwrap_or(blocks.len().saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let kind = ElementKind::Text { text: "A very long title that keeps going on".into(), color: kn_geom::Color::rgb(0, 0, 0), size: 10.0, bold: false, align: kn_model::TextAlign::Start };
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
    fn clock_shows_tenths() {
        assert_eq!(clock(65.25), "1:05.2");
    }
}

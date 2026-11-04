//! The header bar that replaces the system title bar: drag to move,
//! double-click to maximise, window buttons, and the app's main actions.
//! Also resizing from the window edges, since there is no system frame.

use crate::adw::{self, Kind};
use crate::icons::Icon;
use crate::theme::{self, palette};
use crate::{KinettoApp, Pending};
use egui::{Align, CursorIcon, Layout, Sense, Ui, ViewportCommand};

pub const HEIGHT: f32 = 46.0;
/// How close to the window edge the pointer must be to resize.
const EDGE: f32 = 6.0;

impl KinettoApp {
    pub(crate) fn header_bar(&mut self, ui: &mut Ui, frame: &eframe::Frame) {
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();
        // Empty header space moves the window, like a system title bar.
        let drag = ui.interact(rect, ui.scope_id().with("header-drag"), Sense::click_and_drag());
        if drag.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            toggle_maximized(&ctx);
        }

        let editing = !self.choosing_template;
        let (title, subtitle) = self.title_and_status();
        let p = palette(&ctx);
        let galley_title = ui.painter().layout_no_wrap(title, egui::FontId::new(14.0, theme::bold()), p.fg);
        let galley_sub = ui.painter().layout_no_wrap(subtitle, egui::FontId::new(12.0, egui::FontFamily::Proportional), p.dim);
        let block_h = galley_title.size().y + galley_sub.size().y;
        let top = rect.center().y - block_h / 2.0;
        ui.painter().galley(egui::pos2(rect.center().x - galley_title.size().x / 2.0, top), galley_title.clone(), p.fg);
        ui.painter().galley(egui::pos2(rect.center().x - galley_sub.size().x / 2.0, top + galley_title.size().y), galley_sub, p.dim);

        ui.horizontal_centered(|ui| {
            ui.add_space(6.0);
            if editing {
                self.add_menu(ui);
                let undo = self.session.undo_label();
                let tip = undo.map_or("Nothing to undo".to_owned(), |l| format!("Undo {}  (Ctrl+Z)", l.to_lowercase()));
                if ui.add_enabled_ui(undo.is_some(), |ui| adw::icon_button(ui, Icon::Undo, &tip)).inner.clicked() {
                    self.session.undo();
                    self.clamp_selection();
                }
                let redo = self.session.redo_label();
                let tip = redo.map_or("Nothing to redo".to_owned(), |l| format!("Redo {}  (Ctrl+Shift+Z)", l.to_lowercase()));
                if ui.add_enabled_ui(redo.is_some(), |ui| adw::icon_button(ui, Icon::Redo, &tip)).inner.clicked() {
                    self.session.redo();
                    self.clamp_selection();
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(4.0);
                if adw::window_button(ui, Icon::Close, "Close").clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                let (icon, label) = if maximized { (Icon::Restore, "Restore") } else { (Icon::Maximize, "Maximise") };
                if adw::window_button(ui, icon, label).clicked() {
                    toggle_maximized(&ctx);
                }
                if adw::window_button(ui, Icon::Minimize, "Minimise").clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
                }
                ui.add_space(6.0);
                self.main_menu(ui);
                if let Some(output) = self.export_request.take() {
                    self.make(output, frame);
                }
                if editing {
                    if adw::button(ui, Some(Icon::Video), "Make Video", Kind::Suggested).on_hover_text("Save your slides as an MP4 video").clicked() {
                        self.make_video(frame);
                    }
                    let (icon, tip) = if self.playing { (Icon::Stop, "Stop  (Space)") } else { (Icon::Play, "Play  (Space)") };
                    if adw::icon_button(ui, icon, tip).clicked() {
                        self.toggle_play();
                    }
                }
            });
        });
    }

    /// The project name and a word on whether the work is safe.
    fn title_and_status(&self) -> (String, String) {
        if self.choosing_template {
            return ("Kinetto".into(), "Make a video from slides".into());
        }
        let name = self.session.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
        let status = match (&name, self.session.is_dirty()) {
            (None, _) => "Not saved to a file yet",
            (Some(_), true) => "Saving…",
            (Some(_), false) => "All changes saved",
        };
        (name.unwrap_or_else(|| "Untitled video".into()), status.into())
    }

    fn add_menu(&mut self, ui: &mut Ui) {
        let response = adw::icon_button(ui, Icon::Add, "Add text, a shape or a picture");
        egui::Popup::menu(&response).width(340.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            if menu_item(ui, Icon::Text, "Text", None) {
                self.add_text();
            }
            if menu_item(ui, Icon::Picture, "Picture…", None) {
                self.pick_pictures();
            }
            ui.separator();
            ui.label(egui::RichText::new("Shapes").strong());
            ui.label(egui::RichText::new("Choose a shape to add to your slide").small().color(palette(ui.ctx()).dim));
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                egui::Grid::new("shape-gallery").num_columns(4).spacing(egui::vec2(6.0, 6.0)).show(ui, |ui| {
                    for (index, kind) in kn_model::ShapeKind::ALL.into_iter().enumerate() {
                        let response = ui.add_sized([76.0, 74.0], egui::Button::new(""));
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, kind.name()));
                        let rect = egui::Rect::from_center_size(response.rect.center() - egui::vec2(0.0, 9.0), egui::vec2(34.0, 30.0));
                        let p = palette(ui.ctx());
                        if kind == kn_model::ShapeKind::Rectangle {
                            ui.painter().rect_filled(rect, 5.0, p.accent_bg);
                        } else {
                            // A path supports concave shapes such as stars and arrows.
                            let points = kind.points();
                            let mut mesh = egui::epaint::Mesh::default();
                            // Tessellate the shared outline with ear clipping below.
                            paint_shape(ui.painter(), rect, &points, p.accent_bg, &mut mesh);
                        }
                        ui.painter().text(egui::pos2(response.rect.center().x, response.rect.bottom() - 13.0), egui::Align2::CENTER_CENTER, kind.name(), egui::FontId::proportional(10.5), p.fg);
                        if response.on_hover_text(kind.name()).clicked() {
                            self.add_shape(kind);
                            ui.close();
                        }
                        if index % 4 == 3 { ui.end_row(); }
                    }
                });
            });
        });
    }

    fn main_menu(&mut self, ui: &mut Ui) {
        let response = adw::icon_button(ui, Icon::Menu, "Main menu");
        egui::Popup::menu(&response).width(240.0).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            if !self.choosing_template {
                ui.label(egui::RichText::new("Layout").size(12.0).color(palette(ui.ctx()).dim));
                let mut layout = self.settings.layout;
                use crate::settings::Layout;
                if adw::toggle_group(ui, &mut layout, &[(Layout::Simple, "Simple"), (Layout::Timeline, "Timeline")]) {
                    self.set_layout(layout);
                    ui.close();
                }
                ui.separator();
            }
            if menu_item(ui, Icon::Add, "New video", Some("Ctrl+N")) {
                self.request(Pending::New);
            }
            if menu_item(ui, Icon::Open, "Open…", Some("Ctrl+O")) {
                self.pick_open();
            }
            if menu_item(ui, Icon::Copy, "Import PowerPoint or Markdown…", None) {
                self.pick_import();
            }
            if menu_item(ui, Icon::Video, "Quick video…", None) {
                self.open_quick();
            }
            if !self.choosing_template {
                ui.separator();
                if menu_item(ui, Icon::Play, "Present full screen", Some("F5")) {
                    self.start_presenting(ui.ctx());
                }
                if menu_item(ui, Icon::Picture, "Export as GIF…", None) {
                    self.export_request = Some(crate::export::Output::Gif);
                }
                if menu_item(ui, Icon::Text, "Export slides as PDF…", None) {
                    self.export_request = Some(crate::export::Output::Pdf);
                }
                ui.separator();
                if menu_item(ui, Icon::Down, "Save", Some("Ctrl+S")) {
                    self.save();
                }
                if menu_item(ui, Icon::Down, "Save as…", None) {
                    self.save_as();
                }
            }
            ui.separator();
            if menu_item(ui, Icon::More, "About Kinetto", None) {
                self.about = true;
            }
        });
    }

    /// Resizing from the edges and corners, since the window has no frame.
    pub(crate) fn resize_edges(&self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        let r = ctx.content_rect();
        let (left, right) = (pos.x - r.left() < EDGE, r.right() - pos.x < EDGE);
        let (top, bottom) = (pos.y - r.top() < EDGE, r.bottom() - pos.y < EDGE);
        use egui::ResizeDirection as D;
        let (dir, cursor) = match (left, right, top, bottom) {
            (true, _, true, _) => (D::NorthWest, CursorIcon::ResizeNorthWest),
            (_, true, true, _) => (D::NorthEast, CursorIcon::ResizeNorthEast),
            (true, _, _, true) => (D::SouthWest, CursorIcon::ResizeSouthWest),
            (_, true, _, true) => (D::SouthEast, CursorIcon::ResizeSouthEast),
            (true, ..) => (D::West, CursorIcon::ResizeWest),
            (_, true, ..) => (D::East, CursorIcon::ResizeEast),
            (_, _, true, _) => (D::North, CursorIcon::ResizeNorth),
            (_, _, _, true) => (D::South, CursorIcon::ResizeSouth),
            _ => return,
        };
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }

    pub(crate) fn about_dialog(&mut self, ctx: &egui::Context) {
        if !self.about {
            return;
        }
        let body = format!("Version {}\nSlides in, video out.", env!("CARGO_PKG_VERSION"));
        if adw::alert(ctx, "about", "Kinetto", &body, &[("Close", Kind::Normal)]).is_some() {
            self.about = false;
        }
    }
}

fn toggle_maximized(ctx: &egui::Context) {
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
}

/// A menu row with an icon, a label and an optional shortcut hint.
pub(crate) fn menu_item(ui: &mut Ui, icon: Icon, label: &str, shortcut: Option<&str>) -> bool {
    let p = palette(ui.ctx());
    let font = egui::FontId::proportional(14.0);
    let hint_font = egui::FontId::proportional(12.5);
    // Wide enough for the words, so a long label widens the menu instead
    // of running past its edge.
    let text_width = |s: &str, f: &egui::FontId| ui.fonts_mut(|fonts| fonts.layout_no_wrap(s.to_owned(), f.clone(), p.fg).size().x);
    let needed = 36.0 + text_width(label, &font) + shortcut.map_or(12.0, |s| 28.0 + text_width(s, &hint_font));
    let width = ui.available_width().max(needed);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 34.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::MenuItem, true, label));
    if response.hovered() {
        ui.painter().rect_filled(rect, theme::BUTTON_RADIUS, p.button[0]);
    }
    let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + 18.0, rect.center().y), egui::Vec2::splat(16.0));
    crate::icons::paint(ui.painter(), icon_rect, icon, p.fg);
    ui.painter().text(egui::pos2(rect.left() + 36.0, rect.center().y), egui::Align2::LEFT_CENTER, label, font, p.fg);
    if let Some(s) = shortcut {
        ui.painter().text(egui::pos2(rect.right() - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, s, hint_font, p.dim);
    }
    if response.clicked() {
        ui.close();
        return true;
    }
    false
}

/// Triangulate concave outlines so the gallery matches exported geometry.
fn paint_shape(painter: &egui::Painter, rect: egui::Rect, points: &[(f64, f64)], color: egui::Color32, mesh: &mut egui::epaint::Mesh) {
    let mut points: Vec<egui::Pos2> = points.iter().map(|&(x,y)| rect.min + egui::vec2(x as f32 * rect.width(), y as f32 * rect.height())).collect();
    let area: f32 = (0..points.len()).map(|i| { let a = points[i]; let b = points[(i+1)%points.len()]; a.x*b.y-b.x*a.y }).sum();
    if area < 0.0 { points.reverse(); }
    for &point in &points { mesh.colored_vertex(point, color); }
    let mut remaining: Vec<usize> = (0..points.len()).collect();
    let cross = |a: egui::Pos2, b: egui::Pos2, c: egui::Pos2| (b.x-a.x)*(c.y-a.y)-(b.y-a.y)*(c.x-a.x);
    while remaining.len() > 2 {
        let mut found = false;
        for i in 0..remaining.len() {
            let a = remaining[(i+remaining.len()-1)%remaining.len()];
            let b = remaining[i];
            let c = remaining[(i+1)%remaining.len()];
            if cross(points[a], points[b], points[c]) <= 0.0 { continue; }
            if remaining.iter().any(|&j| j != a && j != b && j != c && cross(points[a],points[b],points[j]) >= 0.0 && cross(points[b],points[c],points[j]) >= 0.0 && cross(points[c],points[a],points[j]) >= 0.0) { continue; }
            mesh.add_triangle(a as u32,b as u32,c as u32);
            remaining.remove(i);
            found = true;
            break;
        }
        if !found { break; }
    }
    painter.add(egui::Shape::mesh(std::mem::take(mesh)));
}

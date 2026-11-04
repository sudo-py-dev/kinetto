//! Compact, illustrated transition choices. Hover previews use simple slide
//! diagrams; selecting a tile plays the actual transition in the editor.

use crate::theme::{self, palette};
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use kn_model::Transition;

pub(crate) fn picker(ui: &mut egui::Ui, current: Transition) -> Option<Transition> {
    let mut chosen = None;
    ui.spacing_mut().item_spacing = Vec2::splat(8.0);
    egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
    egui::Grid::new("transition-tiles").num_columns(2).spacing(Vec2::splat(8.0)).show(ui, |ui| {
        for (i, transition) in Transition::ALL.into_iter().enumerate() {
            let p = palette(ui.ctx());
            let selected = transition == current;
            let button = egui::Button::new("")
                .fill(if selected { p.accent_bg.gamma_multiply(0.16) } else { p.card })
                .stroke(Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { p.accent } else { p.border }))
                .corner_radius(10);
            let response = ui.add_sized([120.0, 78.0], button);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, transition.name()));
            let r = response.rect;
            let preview = Rect::from_min_size(r.min + vec2(12.0, 8.0), vec2(96.0, 42.0));
            let start_id = response.id.with("preview-start");
            let progress = if response.hovered() || response.has_focus() {
                let now = ui.input(|input| input.time);
                let start = ui.ctx().data_mut(|data| *data.get_temp_mut_or_insert_with(start_id, || now));
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
                (((now - start) % 1.8 - 0.2) / 1.0).clamp(0.0, 1.0) as f32
            } else {
                ui.ctx().data_mut(|data| data.remove::<f64>(start_id));
                0.38
            };
            preview_transition(ui, preview, transition, progress * progress * (3.0 - 2.0 * progress));
            ui.painter().text(
                pos2(r.center().x, r.bottom() - 17.0),
                Align2::CENTER_CENTER,
                transition.name(),
                FontId::new(12.5, theme::semibold()),
                p.fg,
            );
            if selected {
                let c = pos2(r.right() - 11.0, r.top() + 11.0);
                ui.painter().circle_filled(c, 7.0, p.accent_bg);
                ui.painter().line_segment([c + vec2(-3.0, 0.0), c + vec2(-0.5, 2.5)], Stroke::new(1.5, p.accent_fg));
                ui.painter().line_segment([c + vec2(-0.5, 2.5), c + vec2(3.5, -2.5)], Stroke::new(1.5, p.accent_fg));
            }
            if response.clicked() {
                chosen = Some(transition);
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    });
    chosen
}

fn preview_transition(ui: &egui::Ui, r: Rect, kind: Transition, t: f32) {
    let painter = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let point = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let rectangle = |x: f32, y: f32, w: f32, h: f32| [point(x, y), point(x + w, y), point(x + w, y + h), point(x, y + h)];
    let old = Color32::from_rgb(230, 112, 94);
    let new = palette(ui.ctx()).accent_bg;
    painter.rect_filled(r, 5, Color32::from_rgb(24, 24, 32));
    let face = |quad: [Pos2; 4], color: Color32, alpha: f32, clip: Rect| {
        if alpha <= 0.0 {
            return;
        }
        let painter = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
        painter.add(egui::Shape::convex_polygon(quad.to_vec(), color.gamma_multiply(alpha), Stroke::NONE));
        // A title and two lines make each coloured face read as a slide.
        for (y, end, width) in [(0.32, 0.72, 2.0), (0.56, 0.80, 1.2), (0.72, 0.60, 1.2)] {
            let left = quad[0].lerp(quad[3], y);
            let right = quad[1].lerp(quad[2], y);
            painter.line_segment([left.lerp(right, 0.20), left.lerp(right, end)], Stroke::new(width, Color32::WHITE.gamma_multiply(alpha * 0.8)));
        }
    };
    let full = rectangle(0.0, 0.0, 1.0, 1.0);
    match kind {
        Transition::Cut => face(full, if t < 0.5 { old } else { new }, 1.0, r),
        Transition::Fade => {
            face(full, old, 1.0, r);
            face(full, new, t, r);
        }
        Transition::Dip => {
            face(full, old, (1.0 - 2.0 * t).max(0.0), r);
            face(full, new, (2.0 * t - 1.0).max(0.0), r);
        }
        Transition::Slide => {
            face(rectangle(-t, 0.0, 1.0, 1.0), old, 1.0, r);
            face(rectangle(1.0 - t, 0.0, 1.0, 1.0), new, 1.0, r);
        }
        Transition::SlideUp => {
            face(rectangle(0.0, -t, 1.0, 1.0), old, 1.0, r);
            face(rectangle(0.0, 1.0 - t, 1.0, 1.0), new, 1.0, r);
        }
        Transition::SlideRight => {
            face(rectangle(t, 0.0, 1.0, 1.0), old, 1.0, r);
            face(rectangle(t - 1.0, 0.0, 1.0, 1.0), new, 1.0, r);
        }
        Transition::SlideDown => {
            face(rectangle(0.0, t, 1.0, 1.0), old, 1.0, r);
            face(rectangle(0.0, t - 1.0, 1.0, 1.0), new, 1.0, r);
        }
        Transition::Cover => {
            face(full, old, 1.0, r);
            face(rectangle(1.0 - t, 0.0, 1.0, 1.0), new, 1.0, r);
        }
        Transition::ZoomOut => {
            face(full, old, 1.0, r);
            let inset = 0.1 * (1.0 - t);
            face(rectangle(inset, inset, 1.0 - 2.0 * inset, 1.0 - 2.0 * inset), new, t, r);
        }
        Transition::Wipe => {
            face(full, old, 1.0, r);
            face(full, new, 1.0, Rect::from_min_max(r.min, point(t, 1.0)));
        }
        Transition::Zoom => {
            face(full, old, 1.0, r);
            let extra = 0.2 * (1.0 - t);
            face(rectangle(-extra, -extra, 1.0 + 2.0 * extra, 1.0 + 2.0 * extra), new, t, r);
        }
        Transition::TurnLeft | Transition::TurnRight => {
            let a = t * std::f32::consts::FRAC_PI_2;
            let (leaving, arriving) = (a.cos().max(0.0), a.sin());
            let joint = leaving / (leaving + arriving);
            let mirror = |q: [Pos2; 4]| {
                if kind == Transition::TurnRight {
                    let q = q.map(|v| pos2(r.left() + r.right() - v.x, v.y));
                    [q[1], q[0], q[3], q[2]]
                } else {
                    q
                }
            };
            face(
                mirror([point(0.0, 0.18 * arriving), point(joint, 0.0), point(joint, 1.0), point(0.0, 1.0 - 0.18 * arriving)]),
                old,
                0.5 + 0.5 * leaving,
                r,
            );
            face(
                mirror([point(joint, 0.0), point(1.0, 0.18 * leaving), point(1.0, 1.0 - 0.18 * leaving), point(joint, 1.0)]),
                new,
                0.5 + 0.5 * arriving,
                r,
            );
        }
        Transition::Flip => {
            let across = (t * std::f32::consts::PI).cos();
            let w = across.abs();
            face(rectangle((1.0 - w) / 2.0, 0.05 * (1.0 - w), w, 0.9 + 0.1 * w), if across > 0.0 { old } else { new }, 0.5 + 0.5 * w, r);
        }
    }
}

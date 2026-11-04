//! Draws vello scenes into wgpu textures that egui shows as images. Textures
//! share eframe's device, so there is no copy through the CPU.

use egui_wgpu::RenderState;
use kn_model::Project;
use kn_text::TextEngine;
use kn_time::Tick;
use std::collections::HashMap;
use vello::wgpu;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

/// Which picture a texture holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Surface {
    Stage,
    Thumbnail(usize),
}

pub struct VelloCanvas {
    renderer: Renderer,
    scene: Scene,
    targets: HashMap<Surface, Target>,
}

struct Target {
    size: [u32; 2],
    view: wgpu::TextureView,
    id: egui::TextureId,
    /// What was last drawn; repaint is skipped when it hasn't changed.
    stamp: Option<u64>,
}

pub struct Frame<'a> {
    pub project: &'a Project,
    pub time: Tick,
    /// Physical pixels.
    pub size: [u32; 2],
    /// Identifies the content; `None` always repaints.
    pub stamp: Option<u64>,
}

impl VelloCanvas {
    pub fn new(rs: &RenderState) -> Result<Self, vello::Error> {
        let renderer = Renderer::new(
            &rs.device,
            RendererOptions { antialiasing_support: AaSupport::area_only(), ..Default::default() },
        )?;
        Ok(Self { renderer, scene: Scene::new(), targets: HashMap::new() })
    }

    /// Renders `frame` into `surface` and returns the egui texture holding it.
    pub fn paint(&mut self, rs: &RenderState, surface: Surface, frame: Frame<'_>, text: &mut TextEngine) -> egui::TextureId {
        let size = [frame.size[0].max(1), frame.size[1].max(1)];
        let target = target_for(&mut self.targets, rs, surface, size);
        if frame.stamp.is_some() && target.stamp == frame.stamp {
            return target.id;
        }
        target.stamp = frame.stamp;

        let scale = f64::from(size[0]) / kn_geom::Aspect::CANVAS_WIDTH;
        self.scene.reset();
        kn_render::render_frame(frame.project, frame.time, scale, &mut self.scene, text);
        let params = RenderParams {
            base_color: vello::peniko::Color::TRANSPARENT,
            width: size[0],
            height: size[1],
            antialiasing_method: AaConfig::Area,
        };
        if let Err(err) = self.renderer.render_to_texture(&rs.device, &rs.queue, &self.scene, &target.view, &params) {
            tracing::error!("canvas render failed: {err}");
        }
        target.id
    }

    /// Frees thumbnails for slides that no longer exist.
    pub fn retain_thumbnails(&mut self, rs: &RenderState, count: usize) {
        let mut egui_renderer = rs.renderer.write();
        self.targets.retain(|surface, target| {
            let keep = !matches!(surface, Surface::Thumbnail(i) if *i >= count);
            if !keep {
                egui_renderer.free_texture(&target.id);
            }
            keep
        });
    }
}

fn target_for<'a>(
    targets: &'a mut HashMap<Surface, Target>,
    rs: &RenderState,
    surface: Surface,
    size: [u32; 2],
) -> &'a mut Target {
    if targets.get(&surface).is_some_and(|t| t.size == size) {
        return targets.get_mut(&surface).expect("checked above");
    }
    let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("kinetto canvas"),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut egui_renderer = rs.renderer.write();
    let id = match targets.remove(&surface) {
        Some(old) => {
            egui_renderer.update_egui_texture_from_wgpu_texture(&rs.device, &view, wgpu::FilterMode::Linear, old.id);
            old.id
        }
        None => egui_renderer.register_native_texture(&rs.device, &view, wgpu::FilterMode::Linear),
    };
    targets.entry(surface).or_insert(Target { size, view, id, stamp: None })
}

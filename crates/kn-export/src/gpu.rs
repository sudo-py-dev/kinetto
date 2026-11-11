//! Renders frames on a GPU device of our own (no window) and reads the
//! pixels back, so export runs in the background beside the editor.

use kn_media::MediaLibrary;
use kn_model::Project;
use kn_render::RenderContext;
use kn_time::Tick;
use vello::util::block_on_wgpu;
use vello::wgpu;
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

/// A GPU device to render with. The editor passes its own, so export never
/// creates a second GPU instance beside the one drawing the window (some
/// drivers crash when two are set up and used at once).
#[derive(Clone)]
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// A device of our own, for the command line and tests. Uses only the
    /// main native backends, skipping OpenGL.
    pub fn create() -> Result<Self, GpuError> {
        // Some Vulkan drivers crash when several devices are created at the
        // same moment, so creation is one at a time.
        static CREATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = CREATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(desc);
        pollster::block_on(async {
            let adapter = wgpu::util::initialize_adapter_from_env_or_default(&instance, None).await.map_err(|_| GpuError::NoDevice)?;
            let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor::default()).await.map_err(|_| GpuError::NoDevice)?;
            Ok(Gpu { device, queue })
        })
    }

    /// True when the device is a CPU rasteriser standing in for a graphics
    /// card, like Windows' WARP on CI machines. Vello's compute shaders crash
    /// some of these outright (an access violation, not an error), so tests
    /// treat them as no device at all.
    pub fn is_software(&self) -> bool {
        self.device.adapter_info().device_type == wgpu::DeviceType::Cpu
    }

    /// A device of our own, for tests: `None` when there is no real
    /// graphics card, so GPU tests can skip themselves.
    pub fn create_for_tests() -> Option<Self> {
        Self::create().ok().filter(|gpu| !gpu.is_software())
    }
}

/// A window-less renderer that hands back pixels.
pub struct Offscreen {
    gpu: Gpu,
    renderer: Renderer,
    scene: Scene,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    /// Bytes per row in the readback buffer, padded as wgpu requires.
    padded_row: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("no graphics card is available for rendering")]
    NoDevice,
    #[error("the renderer could not start: {0}")]
    Renderer(#[from] vello::Error),
    #[error("reading the frame back from the graphics card failed")]
    Readback,
}

impl Offscreen {
    pub fn new(width: u32, height: u32) -> Result<Self, GpuError> {
        Self::with_gpu(Gpu::create()?, width, height)
    }

    pub fn with_gpu(gpu: Gpu, width: u32, height: u32) -> Result<Self, GpuError> {
        let device = &gpu.device;
        let renderer = Renderer::new(device, RendererOptions { antialiasing_support: AaSupport::area_only(), ..Default::default() })?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kinetto export"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kinetto readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(Offscreen { gpu, renderer, scene: Scene::new(), texture, view, readback, width, height, padded_row })
    }

    /// Renders the frame at `t` into `rgba` (tightly packed, straight alpha).
    pub fn render(
        &mut self,
        project: &Project,
        media: &MediaLibrary,
        t: Tick,
        cx: &mut RenderContext,
        rgba: &mut Vec<u8>,
    ) -> Result<(), GpuError> {
        let scale = f64::from(self.width) / kn_geom::Aspect::CANVAS_WIDTH;
        self.scene.reset();
        kn_render::render_frame(project, media, t, scale, &mut self.scene, cx);
        let params = RenderParams {
            base_color: vello::peniko::Color::BLACK,
            width: self.width,
            height: self.height,
            antialiasing_method: AaConfig::Area,
        };
        let Gpu { device, queue } = &self.gpu;
        self.renderer.render_to_texture(device, queue, &self.scene, &self.view, &params)?;

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        self.read_pixels(rgba)
    }

    fn read_pixels(&self, rgba: &mut Vec<u8>) -> Result<(), GpuError> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.readback.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        let device = &self.gpu.device;
        block_on_wgpu(device, async {}); // drive the queue
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|_| GpuError::Readback)?;
        rx.recv().map_err(|_| GpuError::Readback)?.map_err(|_| GpuError::Readback)?;

        let row = self.width as usize * 4;
        rgba.clear();
        rgba.reserve(row * self.height as usize);
        {
            let mapped = self.readback.get_mapped_range(..).map_err(|_| GpuError::Readback)?;
            for line in mapped.chunks_exact(self.padded_row as usize) {
                rgba.extend_from_slice(&line[..row]);
            }
        }
        self.readback.unmap();
        Ok(())
    }
}

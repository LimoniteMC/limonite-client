//! The renderer: the GPU device and the window's surface, the camera, and
//! the CPU work that feeds them (textures, tints, meshes).
//!
//! **Capabilities, never vendors** (CLAUDE.md; m4-plan Decision 11). Every
//! decision here reads what the adapter and surface report: limits, formats
//! and present modes. The adapter's name, vendor and device type are logged
//! for context, and no code path reads them.

pub mod camera;
pub mod crack;
pub mod icons;
pub mod mesher;
pub mod meshes;
pub mod outline;
#[cfg(test)]
mod sheet;
pub mod terrain;
pub mod textures;
pub mod animation;
pub mod tint;

use std::sync::Arc;

use limonite_packs::PackStack;
use tracing::{info, warn};
use winit::{event_loop::OwnedDisplayHandle, window::Window};

use self::tint::Tints;

/// What the renderer asks of the GPU.
pub struct Needs {
    /// Texture array layers the block textures take.
    pub texture_layers: u32,
    /// Prefer a present mode without vsync, for frame-time measurements.
    pub uncapped: bool,
}

/// The device, its queue, and the window's surface.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Only the contact sheet (`sheet.rs`) has none: it draws into a texture.
    surface: Option<wgpu::Surface<'static>>,
    pub config: wgpu::SurfaceConfiguration,
    /// The format the UI draws in: the frame's, without the sRGB conversion, where
    /// the frame can have a second view like that (`ui/render.rs`).
    pub ui_format: wgpu::TextureFormat,
}

/// The two ways to draw into one frame's picture.
pub struct Views {
    /// In linear light, converted to sRGB on the way out.
    pub world: wgpu::TextureView,
    /// In `Gpu::ui_format`.
    pub ui: wgpu::TextureView,
}

/// What became of one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Drawn,
    /// Nothing to draw into right now: minimised, hidden, or a timeout.
    Skipped,
}

impl Gpu {
    /// Picks an adapter and creates a device that can present to `window`.
    ///
    /// # Errors
    /// No adapter can present to the window, or one can't meet [`Needs`].
    /// The message names what's missing and both numbers.
    pub fn new(window: Arc<Window>, display: OwnedDisplayHandle, needs: &Needs) -> Result<Self, String> {
        // The display handle ties the instance to the window system, which
        // GLES and Wayland need. `from_env` reads `WGPU_BACKEND`, which
        // forces a backend (`vulkan`, `dx12`, ...) for testing.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(
            Box::new(display),
        ));
        let surface = instance
            .create_surface(window.clone())
            .map_err(|err| format!("can't create a surface for the window: {err}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|err| format!("no GPU adapter can draw to this window: {err}"))?;

        let adapter_info = adapter.get_info();
        info!(
            "GPU adapter: {} ({:?}, {:?}); driver {} {}",
            adapter_info.name,
            adapter_info.backend,
            adapter_info.device_type,
            adapter_info.driver,
            adapter_info.driver_info,
        );

        let (device, queue) = request_device(&adapter, needs)?;
        let layers = adapter.limits().max_texture_array_layers;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| {
                let first = capabilities.formats.first().copied();
                warn!("The surface offers no sRGB format, so colours will be off; using {first:?}");
                first
            })
            .ok_or("the surface offers no formats")?;
        let present_mode = choose_present_mode(&capabilities.present_modes, needs.uncapped);
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("the adapter can't present to this surface")?;
        config.format = format;
        config.present_mode = present_mode;
        let ui_format = plain_view_format(format, adapter.get_downlevel_capabilities().flags, wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
        if ui_format != format {
            config.view_formats.push(ui_format);
        }
        surface.configure(&device, &config);
        info!(
            "Surface: {format:?}, {present_mode:?} (offered {:?}); the UI draws in {ui_format:?}; \
             {} texture array layers allowed",
            capabilities.present_modes, layers
        );
        Ok(Self {
            device,
            queue,
            surface: Some(surface),
            config,
            ui_format,
        })
    }

    /// A device with no window, for drawing into textures of this size: the
    /// contact sheet.
    #[cfg(test)]
    pub fn offscreen(width: u32, height: u32, needs: &Needs) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|err| format!("no GPU adapter: {err}"))?;
        let (device, queue) = request_device(&adapter, needs)?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
        };
        let ui_format = plain_view_format(config.format, adapter.get_downlevel_capabilities().flags, wgpu::DownlevelFlags::VIEW_FORMATS);
        Ok(Self {
            device,
            queue,
            surface: None,
            config,
            ui_format,
        })
    }

    /// Follows the window's new size. A zero size (minimised) is skipped:
    /// a surface can't be zero pixels, and nothing is drawn meanwhile.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        if let Some(surface) = &self.surface {
            surface.configure(&self.device, &self.config);
        }
    }

    /// Acquires the next surface texture, lets `draw` record into it, then
    /// submits and presents. A surface that needs reconfiguring is
    /// reconfigured, and the frame skipped.
    pub fn frame(&mut self, draw: impl FnOnce(&Self, &Views, &mut wgpu::CommandEncoder)) -> Frame {
        let Some(surface) = &self.surface else {
            return Frame::Skipped;
        };
        let (texture, suboptimal) = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Frame::Skipped;
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&self.device, &self.config);
                return Frame::Skipped;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                warn!("Couldn't get the next frame: a validation error (logged by wgpu)");
                return Frame::Skipped;
            }
        };
        let views = Views {
            world: texture.texture.create_view(&wgpu::TextureViewDescriptor::default()),
            ui: texture.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.ui_format),
                ..Default::default()
            }),
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        draw(self, &views, &mut encoder);
        self.queue.submit([encoder.finish()]);
        self.queue.present(texture);
        if suboptimal {
            surface.configure(&self.device, &self.config);
        }
        Frame::Drawn
    }
}

/// The format the UI draws in. An sRGB frame can be viewed without the conversion
/// where the adapter has the capability `needed`, and otherwise it stays as it is.
fn plain_view_format(
    format: wgpu::TextureFormat,
    flags: wgpu::DownlevelFlags,
    needed: wgpu::DownlevelFlags,
) -> wgpu::TextureFormat {
    if flags.contains(needed) { format.remove_srgb_suffix() } else { format }
}

/// Creates a device that meets `needs`.
///
/// The one limit above WebGPU's defaults is a layer per block texture. WebGPU
/// guarantees 256 layers, and desktop adapters report 2,048 or more. So it asks
/// for the adapter's own limit, and fails with numbers if that's too small;
/// smaller adapters wait for M6's tiers.
fn request_device(adapter: &wgpu::Adapter, needs: &Needs) -> Result<(wgpu::Device, wgpu::Queue), String> {
    let layers = adapter.limits().max_texture_array_layers;
    if layers < needs.texture_layers {
        return Err(format!(
            "this GPU's max_texture_array_layers is {layers}, but the block textures need {}",
            needs.texture_layers
        ));
    }
    let required_limits = wgpu::Limits {
        max_texture_array_layers: layers,
        ..wgpu::Limits::default()
    };
    // Everything else M4 uses is core WebGPU: no optional features.
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("limonite"),
        required_features: wgpu::Features::empty(),
        required_limits,
        ..Default::default()
    }))
    .map_err(|err| format!("the GPU adapter wouldn't create a device: {err}"))
}

/// Fifo, vsync, is always supported, and the default. Measuring frame times
/// wants the GPU uncapped: Immediate if the surface offers it, else Mailbox,
/// else Fifo after all.
fn choose_present_mode(offered: &[wgpu::PresentMode], uncapped: bool) -> wgpu::PresentMode {
    let wanted: &[wgpu::PresentMode] = if uncapped {
        &[wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox]
    } else {
        &[]
    };
    wanted
        .iter()
        .copied()
        .find(|mode| offered.contains(mode))
        .unwrap_or(wgpu::PresentMode::Fifo)
}

/// The sky's colour, the clear colour for now: Java's plains sky, #78A7FF,
/// in linear light for an sRGB surface.
pub const SKY: wgpu::Color = wgpu::Color {
    r: 0.188,
    g: 0.386,
    b: 1.0,
    a: 1.0,
};

/// Loads the biome tints, logging the colormaps, the climate table and the
/// water colour.
pub fn load_tints(stack: &PackStack) -> Tints {
    let (tints, report) = Tints::load(stack);
    let maps: Vec<String> = report
        .colormaps
        .iter()
        .map(|(path, size)| {
            let name = path.rsplit('/').next().unwrap_or(path);
            match size {
                Ok((width, height)) => format!("{name} {width}×{height}"),
                Err(err) => {
                    warn!("Colormap {path} didn't load, so its tint is white: {err}");
                    format!("{name} missing")
                }
            }
        })
        .collect();
    let water = match &report.water {
        Ok([r, g, b, a]) => format!("#{r:02X}{g:02X}{b:02X} at {:.0}% opacity", f32::from(*a) / 2.55),
        Err(err) => {
            warn!("No water colour, so water is white: {err}");
            "missing".to_string()
        }
    };
    info!(
        "Tints: {} Bedrock biome ids with a climate ({} Java biomes share an id with a lower one); \
         colormaps {}; water {water}",
        report.biomes,
        report.collisions,
        maps.join(", "),
    );
    tints
}

#[cfg(test)]
mod tests {
    use wgpu::PresentMode;

    use super::*;

    #[test]
    fn the_ui_blends_in_gamma_where_the_frame_can_be_viewed_that_way() {
        use wgpu::{DownlevelFlags as Flags, TextureFormat::Bgra8Unorm, TextureFormat::Bgra8UnormSrgb};
        let needed = Flags::SURFACE_VIEW_FORMATS;
        assert_eq!(plain_view_format(Bgra8UnormSrgb, needed, needed), Bgra8Unorm);
        assert_eq!(plain_view_format(Bgra8UnormSrgb, Flags::empty(), needed), Bgra8UnormSrgb);
        assert_eq!(plain_view_format(Bgra8Unorm, needed, needed), Bgra8Unorm);
    }

    #[test]
    fn present_modes_fall_back_to_fifo() {
        let all = [PresentMode::Fifo, PresentMode::Mailbox, PresentMode::Immediate];
        assert_eq!(choose_present_mode(&all, false), PresentMode::Fifo);
        assert_eq!(choose_present_mode(&all, true), PresentMode::Immediate);
        assert_eq!(choose_present_mode(&[PresentMode::Fifo, PresentMode::Mailbox], true), PresentMode::Mailbox);
        assert_eq!(choose_present_mode(&[PresentMode::Fifo], true), PresentMode::Fifo);
    }
}

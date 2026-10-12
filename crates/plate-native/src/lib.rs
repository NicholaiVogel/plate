//! Thin safe wrappers over OpenColorIO and OpenImageIO.
//!
//! Both libraries are C++, so they sit behind a small `cxx` shim in `cpp/shim.cpp`.
//! Everything above this crate is Rust.

use anyhow::Result;
use std::sync::Arc;

#[cxx::bridge(namespace = "plate")]
mod ffi {
    struct OiioProbe {
        width: u32,
        height: u32,
        channels: u32,
        format: String,
        bits: u32,
        colorspace: String,
        pixel_aspect: f32,
        has_alpha: bool,
        /// From the FramesPerSecond attribute; 0 when absent.
        fps: f32,
    }

    struct OiioPart {
        subimage: u32,
        name: String,
        channels: Vec<String>,
    }

    struct OiioReadInfo {
        has_alpha: bool,
        pixel_aspect: f32,
    }

    unsafe extern "C++" {
        include!("plate-native/cpp/shim.h");

        type Config;
        type Processor;

        fn config_from_file(path: &str) -> Result<UniquePtr<Config>>;
        fn config_from_env() -> Result<UniquePtr<Config>>;
        fn config_name(cfg: &Config) -> String;
        fn config_description(cfg: &Config) -> String;
        fn colorspace_names(cfg: &Config) -> Vec<String>;
        fn colorspace_family(cfg: &Config, name: &str) -> String;
        fn colorspace_description(cfg: &Config, name: &str) -> String;
        fn colorspace_is_data(cfg: &Config, name: &str) -> bool;
        fn colorspace_encoding(cfg: &Config, name: &str) -> String;
        fn role_colorspace(cfg: &Config, role: &str) -> String;
        fn displays(cfg: &Config) -> Vec<String>;
        fn default_display(cfg: &Config) -> String;
        fn views(cfg: &Config, display: &str) -> Vec<String>;
        fn default_view(cfg: &Config, display: &str) -> String;

        fn display_processor(
            cfg: &Config,
            src: &str,
            display: &str,
            view: &str,
            exposure_stops: f32,
            gamma: f32,
        ) -> Result<UniquePtr<Processor>>;
        fn colorspace_processor(cfg: &Config, src: &str, dst: &str)
            -> Result<UniquePtr<Processor>>;
        fn apply_rgba(proc: &Processor, pixels: &mut [f32], width: usize, height: usize)
            -> Result<()>;
        fn processor_is_noop(proc: &Processor) -> bool;

        fn oiio_probe(path: &str) -> Result<OiioProbe>;
        fn oiio_read_rgba_half(path: &str, out: &mut [u16], width: u32, height: u32)
            -> Result<OiioReadInfo>;
        fn oiio_extensions() -> Vec<String>;
        fn oiio_parts(path: &str) -> Result<Vec<OiioPart>>;
        fn oiio_read_channels_half(
            path: &str,
            subimage: u32,
            map: &[i32],
            out: &mut [u16],
            width: u32,
            height: u32,
        ) -> Result<OiioReadInfo>;
    }
}

pub use ffi::{OiioPart, OiioProbe, OiioReadInfo};

/// The OCIO config every fresh install starts with.
pub const BUILTIN_STUDIO_CONFIG: &str = "ocio://studio-config-latest";

#[derive(Debug, Clone)]
pub struct ColorSpaceInfo {
    pub name: String,
    pub family: String,
    pub description: String,
    /// OCIO 2 encoding: scene-linear, display-linear, log, sdr-video, hdr-video, data, or empty.
    pub encoding: String,
    pub is_data: bool,
}

/// A loaded OCIO config. OCIO configs are immutable once loaded and safe to
/// share across threads.
#[derive(Clone)]
pub struct OcioConfig {
    inner: Arc<cxx::UniquePtr<ffi::Config>>,
    source: String,
}

// SAFETY: OCIO's ConstConfigRcPtr is a shared_ptr to an immutable config; all
// accessors used here are const and documented as thread-safe.
unsafe impl Send for OcioConfig {}
unsafe impl Sync for OcioConfig {}

impl OcioConfig {
    pub fn from_file(path: &str) -> Result<Self> {
        let inner = ffi::config_from_file(path)?;
        Ok(Self { inner: Arc::new(inner), source: path.to_string() })
    }

    pub fn from_env() -> Result<Self> {
        let inner = ffi::config_from_env()?;
        let source = std::env::var("OCIO").unwrap_or_default();
        Ok(Self { inner: Arc::new(inner), source })
    }

    pub fn builtin() -> Result<Self> {
        Self::from_file(BUILTIN_STUDIO_CONFIG)
    }

    fn cfg(&self) -> &ffi::Config {
        self.inner.as_ref().as_ref().expect("null OCIO config")
    }

    /// The path or URI the config was loaded from.
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn name(&self) -> String {
        ffi::config_name(self.cfg())
    }

    pub fn description(&self) -> String {
        ffi::config_description(self.cfg())
    }

    pub fn colorspaces(&self) -> Vec<ColorSpaceInfo> {
        ffi::colorspace_names(self.cfg())
            .into_iter()
            .map(|name| ColorSpaceInfo {
                family: ffi::colorspace_family(self.cfg(), &name),
                description: ffi::colorspace_description(self.cfg(), &name),
                encoding: ffi::colorspace_encoding(self.cfg(), &name),
                is_data: ffi::colorspace_is_data(self.cfg(), &name),
                name,
            })
            .collect()
    }

    pub fn encoding(&self, name: &str) -> String {
        ffi::colorspace_encoding(self.cfg(), name)
    }

    pub fn has_colorspace(&self, name: &str) -> bool {
        !ffi::role_colorspace(self.cfg(), name).is_empty()
    }

    /// Resolves a role (or alias, or colorspace name) to a colorspace name.
    pub fn resolve(&self, name_or_role: &str) -> Option<String> {
        let name = ffi::role_colorspace(self.cfg(), name_or_role);
        (!name.is_empty()).then_some(name)
    }

    pub fn displays(&self) -> Vec<String> {
        ffi::displays(self.cfg())
    }

    pub fn default_display(&self) -> String {
        ffi::default_display(self.cfg())
    }

    pub fn views(&self, display: &str) -> Vec<String> {
        ffi::views(self.cfg(), display)
    }

    pub fn default_view(&self, display: &str) -> String {
        ffi::default_view(self.cfg(), display)
    }

    pub fn display_processor(
        &self,
        src: &str,
        display: &str,
        view: &str,
        exposure_stops: f32,
        gamma: f32,
    ) -> Result<OcioProcessor> {
        let p = ffi::display_processor(self.cfg(), src, display, view, exposure_stops, gamma)?;
        Ok(OcioProcessor { inner: Arc::new(p) })
    }

    pub fn colorspace_processor(&self, src: &str, dst: &str) -> Result<OcioProcessor> {
        let p = ffi::colorspace_processor(self.cfg(), src, dst)?;
        Ok(OcioProcessor { inner: Arc::new(p) })
    }
}

/// An optimized OCIO CPU processor for packed f32 RGBA.
#[derive(Clone)]
pub struct OcioProcessor {
    inner: Arc<cxx::UniquePtr<ffi::Processor>>,
}

// SAFETY: OCIO CPUProcessor::apply is const and thread-safe.
unsafe impl Send for OcioProcessor {}
unsafe impl Sync for OcioProcessor {}

impl OcioProcessor {
    fn proc(&self) -> &ffi::Processor {
        self.inner.as_ref().as_ref().expect("null OCIO processor")
    }

    pub fn is_noop(&self) -> bool {
        ffi::processor_is_noop(self.proc())
    }

    /// Applies the transform in place. `pixels.len()` must be at least `width * height * 4`.
    pub fn apply_rgba(&self, pixels: &mut [f32], width: usize, height: usize) -> Result<()> {
        ffi::apply_rgba(self.proc(), pixels, width, height)?;
        Ok(())
    }
}

/// Reads image metadata via OpenImageIO without decoding pixels.
pub fn oiio_probe(path: &str) -> Result<OiioProbe> {
    Ok(ffi::oiio_probe(path)?)
}

/// Decodes the first subimage straight into a caller-owned half-float RGBA
/// buffer sized for the display window. `out` holds raw f16 bits.
pub fn oiio_read_rgba_half(path: &str, out: &mut [u16], width: u32, height: u32) -> Result<OiioReadInfo> {
    Ok(ffi::oiio_read_rgba_half(path, out, width, height)?)
}

/// Channel names of every part (subimage) in a file, without reading pixels.
pub fn oiio_parts(path: &str) -> Result<Vec<OiioPart>> {
    Ok(ffi::oiio_parts(path)?)
}

/// Reads selected channels of one part into half-float RGBA. `map[k]` is the
/// channel index feeding R, G, B, A, or -1 (zero, or one for alpha).
pub fn oiio_read_channels_half(path: &str, subimage: u32, map: [i32; 4], out: &mut [u16], width: u32, height: u32) -> Result<OiioReadInfo> {
    Ok(ffi::oiio_read_channels_half(path, subimage, &map, out, width, height)?)
}

/// Every file extension OpenImageIO has a reader for, lowercase without dots.
pub fn oiio_extensions() -> Vec<String> {
    ffi::oiio_extensions()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_config_loads_and_transforms() {
        let cfg = OcioConfig::builtin().expect("builtin config");
        assert!(!cfg.colorspaces().is_empty());
        let display = cfg.default_display();
        let view = cfg.default_view(&display);
        let src = cfg.resolve("scene_linear").expect("scene_linear role");
        let proc = cfg.display_processor(&src, &display, &view, 0.0, 1.0).unwrap();
        let mut px = vec![0.18f32, 0.18, 0.18, 1.0];
        proc.apply_rgba(&mut px, 1, 1).unwrap();
        assert!(px[0] > 0.2 && px[0] < 0.7, "mid grey mapped to {px:?}");
    }

    #[test]
    fn oiio_knows_exr_and_dpx() {
        let exts = oiio_extensions();
        assert!(exts.iter().any(|e| e == "exr"));
        assert!(exts.iter().any(|e| e == "dpx"));
    }
}

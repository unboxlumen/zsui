use std::sync::{Arc, Mutex};

use crate::linux_direct_menu::LinuxMenuTextMetrics;
use crate::{
    Color, HorizontalAlign, NativeTypographyProfile, Rect, Size, TextRole, TextStyle, TextWeight,
    TextWrap, VerticalAlign, ZsPlatformStyle, ZsRustTextEngine, ZsTextPixelRect,
    ZsTextRasterProfile,
};

pub(crate) const LINUX_RUST_TEXT_TYPOGRAPHY_SOURCE: &str = "zsui_rust_text_fontdb";

#[cfg(feature = "bundled-ui-font")]
use crate::bundled_font;

/// Shared Linux text system for the Cairo and tiny-skia presenters.
///
/// Both Linux Direct renderers use the same ZSUI HarfRust/Swash engine as the
/// Win32 renderer. The presenter only supplies a pixel buffer; shaping,
/// fallback, metrics and glyph rasters stay identical across profiles.
#[derive(Clone)]
pub(crate) struct LinuxRustTextSystem {
    inner: Arc<Mutex<ZsRustTextEngine>>,
    ui_family: Arc<str>,
    ui_scale: f32,
}

pub(crate) struct LinuxRasterizedText {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    /// Premultiplied BGRA pixels, ready for an ARGB32 surface or softbuffer.
    pub pixels: Vec<u8>,
    /// Logical placement of the raster's top-left corner.
    pub x: f32,
    pub y: f32,
}

impl LinuxRustTextSystem {
    pub(crate) fn new(configured_font: &str) -> Self {
        // The Linux Direct Fluent profile uses the same 14/20 type scale as
        // Win32 instead of scaling the whole UI by the host 11 pt setting.
        // The family still comes from fontconfig.
        let (configured_family, _logical_pixels) = parse_configured_font(configured_font);
        #[cfg(feature = "bundled-ui-font")]
        let (family, engine) = {
            let _ = configured_family;
            (
                bundled_font::FAMILY.to_string(),
                ZsRustTextEngine::new()
                    .with_preferred_font(bundled_font::FAMILY, bundled_font::DATA.to_vec()),
            )
        };
        #[cfg(not(feature = "bundled-ui-font"))]
        let (family, engine) = (configured_family, ZsRustTextEngine::new());
        Self {
            inner: Arc::new(Mutex::new(engine)),
            ui_family: Arc::from(family),
            ui_scale: 1.0,
        }
    }

    pub(crate) fn ui_family(&self) -> &str {
        &self.ui_family
    }

    pub(crate) const fn ui_scale(&self) -> f32 {
        self.ui_scale
    }

    pub(crate) fn measure(&self, text: &str, style: &TextStyle, max_width: Option<f32>) -> Size {
        if text.is_empty() {
            return Size {
                width: 0,
                height: 0,
            };
        }
        let Some(mut engine) = self.inner.lock().ok() else {
            return Size {
                width: 0,
                height: 0,
            };
        };
        engine
            .layout(
                text,
                style,
                max_width.map_or(0, |width| width.ceil().max(0.0) as i32),
                0,
                1.0,
            )
            .size()
    }

    pub(crate) fn typography_profile(
        &self,
        typography_scale: f32,
        rasterization: &'static str,
    ) -> NativeTypographyProfile {
        let family = self.ui_family.to_string();
        let mut profile = NativeTypographyProfile::new(
            ZsPlatformStyle::current(),
            LINUX_RUST_TEXT_TYPOGRAPHY_SOURCE,
            family.clone(),
            "Monospace",
            family,
            typography_scale,
            rasterization,
        )
        .with_configured_ui_font(if cfg!(feature = "bundled-ui-font") {
            "Inter (bundled)"
        } else {
            crate::linux_direct::linux_direct_configured_font_name()
        });
        let body = profile.body_metrics;
        let style = TextStyle {
            font_family: profile.ui_font_family.clone(),
            size: body.size,
            line_height: body.line_height,
            semantic_role: Some(TextRole::Body),
            weight: TextWeight::Regular,
            color: Color::rgb(0, 0, 0),
            horizontal_align: HorizontalAlign::Start,
            vertical_align: VerticalAlign::Center,
            wrap: TextWrap::NoWrap,
            ellipsis: false,
        };
        if let Ok(mut engine) = self.inner.lock() {
            let layout = engine.layout("Hg", &style, 0, 0, 1.0);
            if let Some(line) = layout.lines().first() {
                let ascent = line.baseline_px - line.top_px;
                let descent = (line.top_px + line.height_px - line.baseline_px).max(0.0);
                let leading = (body.line_height - ascent - descent).max(0.0);
                profile = profile.with_body_vertical_metrics(ascent, descent, leading);
            }
        }
        profile
    }

    /// Rasterizes one text command into a standalone premultiplied BGRA buffer.
    ///
    /// The Cairo presenter keeps drawing primitives in Cairo and only uses this
    /// surface for glyphs. The surface covers the command bounds so descenders
    /// and CJK overhang are not clipped by the layout's content box.
    pub(crate) fn rasterize(
        &self,
        text: &str,
        style: &TextStyle,
        bounds: Rect,
        scale_factor: f32,
        origin_y: i32,
    ) -> Option<LinuxRasterizedText> {
        if !cfg!(target_endian = "little") {
            // The Cairo ARGB32 binding uses native-endian pixels; the Swash
            // compositor writes BGRA. Keep the existing little-endian Linux
            // contract rather than emitting channel-swapped glyphs.
            return None;
        }
        if text.is_empty() || bounds.width <= 0 || bounds.height <= 0 {
            return None;
        }
        let scale = scale_factor.max(0.1);
        let width_px = (bounds.width.max(0) as f32 * scale).round().max(1.0) as i32;
        let height_px = (bounds.height.max(0) as f32 * scale).round().max(1.0) as i32;
        let constrain_width = style.wrap == TextWrap::Word || style.ellipsis;
        let mut engine = self.inner.lock().ok()?;
        let layout = engine.layout(
            text,
            style,
            if constrain_width { width_px } else { 0 },
            height_px,
            scale,
        );
        let size = layout.size();
        let text_width = size.width.max(1);
        let x_offset = if constrain_width {
            0
        } else {
            match style.horizontal_align {
                HorizontalAlign::Start => 0,
                HorizontalAlign::Center => (width_px - text_width).max(0) / 2,
                HorizontalAlign::End => (width_px - text_width).max(0),
            }
        };
        let y_offset = 0;
        let stride = width_px as usize * 4;
        let mut pixels = vec![0u8; stride.saturating_mul(height_px as usize)];
        engine
            .composite_bgra_clipped(
                &layout,
                &mut pixels,
                width_px as u32,
                height_px as u32,
                stride,
                x_offset,
                y_offset,
                ZsTextPixelRect::new(0, 0, width_px as u32, height_px as u32),
                style.color,
                ZsTextRasterProfile::grayscale(),
            )
            .ok()?;
        Some(LinuxRasterizedText {
            width: width_px as u32,
            height: height_px as u32,
            stride,
            pixels,
            x: bounds.x as f32,
            y: bounds.y.saturating_add(origin_y) as f32,
        })
    }

    #[cfg(feature = "text-input-core")]
    pub(crate) fn shape_line(
        &self,
        text: &str,
        typography_scale: f32,
    ) -> Option<crate::native_input_visuals::NativeShapedTextLine> {
        if text.is_empty() {
            return None;
        }
        let body = TextRole::Body.metrics_for(ZsPlatformStyle::current());
        let mut style = TextStyle::line(
            self.ui_family.to_string(),
            body.size * typography_scale,
            Color::rgb(0, 0, 0),
        );
        style.line_height = body.line_height * typography_scale;
        style.ellipsis = false;
        style.semantic_role = Some(TextRole::Body);
        let mut engine = self.inner.lock().ok()?;
        let layout = engine.layout(text, &style, 0, 0, 1.0);
        crate::rust_text_renderer::native_shaped_line_from_layout(text, &layout, style.size)
    }
}

impl LinuxMenuTextMetrics for LinuxRustTextSystem {
    fn measure_menu_text(&self, text: &str) -> (i32, i32) {
        let mut style = TextStyle::line(
            self.ui_family().to_string(),
            14.0 * self.ui_scale(),
            Color::rgb(0, 0, 0),
        );
        style.line_height = 20.0 * self.ui_scale();
        style.ellipsis = false;
        let size = self.measure(text, &style, None);
        (size.width, size.height)
    }
}

pub(crate) fn parse_configured_font(configured: &str) -> (String, f32) {
    let configured = configured.trim().trim_matches('\'');
    let mut parts = configured.rsplitn(2, char::is_whitespace);
    let possible_size = parts.next().unwrap_or_default();
    let family = parts.next().unwrap_or(configured).trim();
    let point_size = possible_size.parse::<f32>().ok().filter(|size| *size > 0.0);
    let family = if point_size.is_some() && !family.is_empty() {
        family
    } else {
        configured
    };
    (
        if family.is_empty() { "Sans" } else { family }.to_string(),
        point_size.map(|size| size * (96.0 / 72.0)).unwrap_or(14.0),
    )
}

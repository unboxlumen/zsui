//! Optional native-window integration policies.

use serde::{Deserialize, Serialize};

/// Actual system caption reservation expressed in client-area physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptionMetrics {
    /// The rectangle occupied by system caption buttons.
    pub buttons: crate::Rect,
    /// Window scale at which the rectangle was measured.
    pub dpi: u32,
    /// Whether the requested system backdrop was accepted.
    pub backdrop_active: bool,
}

impl CaptionMetrics {
    /// Width that View content must reserve in logical units.
    pub fn reserved_width(self) -> crate::Dp {
        crate::Dp::new(self.buttons.width.max(0) as f32 * 96.0 / self.dpi.max(96) as f32)
    }
}

/// Physical bounds, work area and scale for one native window.
///
/// `bounds` and `work_area` use desktop physical pixels, with the primary
/// monitor's top-left corner as the origin. `dpi` is the window's current
/// scale in DPI units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowMetrics {
    /// Current outer window bounds.
    pub bounds: crate::Rect,
    /// Monitor work area that excludes panels, docks and taskbars.
    pub work_area: crate::Rect,
    /// Current window DPI.
    pub dpi: u32,
}

impl WindowMetrics {
    /// Logical scale factor derived from `dpi`.
    pub fn scale_factor(self) -> f32 {
        self.dpi.max(96) as f32 / 96.0
    }
}

/// Typed notifications emitted by the native window's integration services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeWindowEvent {
    /// A registered system-wide shortcut fired.
    GlobalHotkey(crate::Command),
    /// The active system/theme palette changed.
    ThemeChanged(crate::PlatformPalette),
    /// Caption geometry changed after creation, resizing, or a DPI transition.
    CaptionChanged(CaptionMetrics),
    /// Window bounds, monitor work area or DPI changed.
    MetricsChanged(WindowMetrics),
    /// Desktop integration was re-established after the system shell restarted.
    DesktopRecreated,
    /// A recoverable native service operation failed.
    Error { operation: String, message: String },
}

/// The backdrop requested for an integrated title bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowBackdrop {
    /// An opaque, theme-resolved background.
    #[default]
    Solid,
    /// The system's Mica backdrop, with an opaque fallback when unavailable.
    Mica,
}

/// A system-caption title bar whose remaining area belongs to the View.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegratedTitleBar {
    height_milli_dp: u32,
    /// The desired system backdrop.
    pub backdrop: WindowBackdrop,
}

impl IntegratedTitleBar {
    /// Creates a title bar with a finite height between 24 and 256 DP.
    pub fn new(height: crate::Dp) -> crate::ZsuiResult<Self> {
        if !height.0.is_finite() || !(24.0..=256.0).contains(&height.0) {
            return Err(crate::ZsuiError::invalid_spec(
                "title_bar.height",
                "height must be between 24 and 256 DP",
            ));
        }
        Ok(Self {
            height_milli_dp: (height.0 * 1000.0).round() as u32,
            backdrop: WindowBackdrop::Solid,
        })
    }

    /// Requests a system backdrop with an opaque fallback.
    pub const fn backdrop(mut self, backdrop: WindowBackdrop) -> Self {
        self.backdrop = backdrop;
        self
    }

    /// Returns the logical height of the title bar.
    pub fn height(self) -> crate::Dp {
        crate::Dp::new(self.height_milli_dp as f32 / 1000.0)
    }
}

impl Default for IntegratedTitleBar {
    fn default() -> Self {
        Self {
            height_milli_dp: 40_000,
            backdrop: WindowBackdrop::Solid,
        }
    }
}

/// How a per-pixel alpha surface participates in pointer hit testing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlphaHitTest {
    /// Fully transparent pixels pass pointer input to the underlying window.
    #[default]
    TransparentPixels,
    /// All pixels pass pointer input to the underlying window.
    PassThrough,
}

/// The presentation surface used by a native window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowSurface {
    /// The normal buffered, opaque window surface.
    #[default]
    Opaque,
    /// A premultiplied-alpha surface presented atomically by the host.
    PerPixelAlpha { hit_test: AlphaHitTest },
}

/// The native stacking and activation policy of a window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowLayer {
    /// A normal application window.
    #[default]
    Normal,
    /// A non-activating desktop surface, below normal application windows.
    Desktop { follow_show_desktop: bool },
    /// A topmost overlay such as a desktop pet or HUD.
    ///
    /// Overlays are always placed above normal windows and are always
    /// undecorated. `activate_on_click=false` keeps them from taking focus.
    Overlay { activate_on_click: bool },
}

impl WindowLayer {
    /// Whether this layer is a topmost overlay.
    pub const fn is_overlay(self) -> bool {
        matches!(self, Self::Overlay { .. })
    }
}

/// Windows rasterization policy. The default preserves the GDI paint path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowRenderer {
    /// Buffered GDI/GDI+ with the shared text engine.
    #[default]
    Gdi,
    /// Direct2D's software render target.
    Direct2dSoftware,
    /// Direct2D's hardware render target, explicitly requested by the application.
    Direct2dHardware,
}

/// Declarative integration options; native handles never enter this structure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowIntegration {
    /// Optional integrated system title bar.
    pub title_bar: Option<IntegratedTitleBar>,
    /// Surface alpha and hit-testing policy.
    pub surface: WindowSurface,
    /// Window stacking policy.
    pub layer: WindowLayer,
    /// Rasterization policy.
    pub renderer: WindowRenderer,
}

impl WindowIntegration {
    pub(crate) fn validate(&self, decorations: bool, always_on_top: bool) -> crate::ZsuiResult<()> {
        if let Some(title) = self.title_bar {
            IntegratedTitleBar::new(title.height())?;
            if !decorations || self.surface != WindowSurface::Opaque {
                return Err(crate::ZsuiError::invalid_spec(
                    "title_bar",
                    "integrated system captions require an opaque decorated window",
                ));
            }
        }
        if matches!(self.layer, WindowLayer::Desktop { .. }) && (decorations || always_on_top) {
            return Err(crate::ZsuiError::invalid_spec(
                "window.layer",
                "desktop windows must be undecorated and cannot be always-on-top",
            ));
        }
        if self.layer.is_overlay() && decorations {
            return Err(crate::ZsuiError::invalid_spec(
                "window.layer",
                "overlay windows must be undecorated",
            ));
        }
        if self.layer.is_overlay() && self.title_bar.is_some() {
            return Err(crate::ZsuiError::invalid_spec(
                "window.layer",
                "overlay windows cannot use an integrated system title bar",
            ));
        }
        if self.surface != WindowSurface::Opaque && decorations {
            return Err(crate::ZsuiError::invalid_spec(
                "window.surface",
                "per-pixel alpha windows must be undecorated",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_height_and_surface_combinations_are_validated() {
        for height in [f32::NAN, f32::INFINITY, -1.0, 0.0, 257.0] {
            assert!(IntegratedTitleBar::new(crate::Dp::new(height)).is_err());
        }
        let mut options = WindowIntegration {
            title_bar: Some(IntegratedTitleBar::default()),
            ..Default::default()
        };
        assert!(options.validate(true, false).is_ok());
        assert!(options.validate(false, false).is_err());
        options.surface = WindowSurface::PerPixelAlpha {
            hit_test: AlphaHitTest::TransparentPixels,
        };
        assert!(options.validate(true, false).is_err());
        options.title_bar = None;
        options.layer = WindowLayer::Desktop {
            follow_show_desktop: true,
        };
        assert!(options.validate(false, false).is_ok());
        assert!(options.validate(false, true).is_err());
    }

    #[test]
    fn overlay_layer_requires_an_undecorated_window() {
        let mut options = WindowIntegration {
            layer: WindowLayer::Overlay {
                activate_on_click: false,
            },
            ..Default::default()
        };
        assert!(options.validate(false, false).is_ok());
        assert!(options.validate(true, false).is_err());
        options.layer = WindowLayer::Overlay {
            activate_on_click: true,
        };
        options.surface = WindowSurface::PerPixelAlpha {
            hit_test: AlphaHitTest::TransparentPixels,
        };
        assert!(options.validate(false, false).is_ok());
        options.title_bar = Some(IntegratedTitleBar::default());
        assert!(options.validate(false, false).is_err());
    }

    #[test]
    fn window_metrics_reports_logical_scale() {
        let metrics = WindowMetrics {
            bounds: crate::Rect {
                x: -100,
                y: -50,
                width: 200,
                height: 100,
            },
            work_area: crate::Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            dpi: 144,
        };
        assert!((metrics.scale_factor() - 1.5).abs() < 0.001);
    }
}

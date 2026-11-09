use serde::{Deserialize, Serialize};

use crate::capability::{CapabilityStatus, CapabilitySupport, HostCapabilities};
use crate::components::UiNode;
use crate::menu::MenuSpec;

/// A logical position in the virtual desktop.
///
/// The origin is the top-left of the primary monitor, with `y` growing
/// downward. Native hosts convert this to their platform coordinate space
/// using the target monitor's DPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowPosition {
    x_milli_dp: i32,
    y_milli_dp: i32,
}

impl WindowPosition {
    /// Creates a position from logical DP coordinates.
    pub fn new(x: crate::Dp, y: crate::Dp) -> Self {
        Self {
            x_milli_dp: dp_to_milli(x),
            y_milli_dp: dp_to_milli(y),
        }
    }

    /// Creates a position from milli-DP values, matching the serialized form.
    pub const fn from_milli_dp(x_milli_dp: i32, y_milli_dp: i32) -> Self {
        Self {
            x_milli_dp,
            y_milli_dp,
        }
    }

    /// The horizontal logical coordinate.
    pub fn x(self) -> crate::Dp {
        crate::Dp::new(self.x_milli_dp as f32 / 1000.0)
    }

    /// The vertical logical coordinate.
    pub fn y(self) -> crate::Dp {
        crate::Dp::new(self.y_milli_dp as f32 / 1000.0)
    }

    /// Converts this position to physical pixels for `dpi`.
    pub fn to_physical(self, dpi: crate::Dpi) -> (i32, i32) {
        (
            self.x().to_px(dpi).round_i32(),
            self.y().to_px(dpi).round_i32(),
        )
    }
}

fn dp_to_milli(value: crate::Dp) -> i32 {
    if !value.0.is_finite() {
        return 0;
    }
    (value.0 * 1000.0)
        .round()
        .clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSpec {
    pub title: String,
    pub width: u32,
    pub height: u32,
    /// Optional initial logical position in the virtual desktop.
    #[serde(default)]
    pub position: Option<WindowPosition>,
    pub min_width: Option<u32>,
    pub min_height: Option<u32>,
    pub visible: bool,
    pub resizable: bool,
    pub decorations: bool,
    /// The application draws the title bar inside the client area; the
    /// platform keeps resize borders, snapping and the taskbar entry.
    #[serde(default)]
    pub custom_title_bar: bool,
    pub always_on_top: bool,
    pub transparent: bool,
    /// Optional host integration policies, with conservative defaults.
    #[serde(default)]
    pub integration: crate::WindowIntegration,
    /// Global keyboard bindings owned by this native window.
    #[serde(default)]
    pub global_hotkeys: Vec<crate::HotkeySpec>,
    /// Safe native child declarations for this window.
    #[cfg(feature = "native-host")]
    #[serde(default)]
    pub native_hosts: crate::NativeHosts,
    /// Per-window native providers; runtime objects are never serialized.
    #[cfg(all(windows, feature = "windows-win32", feature = "native-host"))]
    #[serde(skip)]
    pub windows_hooks: Option<crate::windows_win32_host::interop::WindowsWindowHooks>,
    pub icon_path: Option<String>,
    pub menu: Option<MenuSpec>,
    pub content: Option<UiNode>,
}

pub type Window = WindowSpec;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowNativeOptions {
    pub position: Option<WindowPosition>,
    pub min_width: Option<u32>,
    pub min_height: Option<u32>,
    pub resizable: bool,
    pub decorations: bool,
    pub always_on_top: bool,
    pub transparent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowResolvedSpec {
    pub requested: WindowSpec,
    pub effective: WindowSpec,
    pub degraded_capabilities: Vec<String>,
}

impl WindowSpec {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: 900,
            height: 620,
            position: None,
            min_width: None,
            min_height: None,
            visible: true,
            resizable: true,
            decorations: true,
            custom_title_bar: false,
            always_on_top: false,
            transparent: false,
            integration: crate::WindowIntegration::default(),
            global_hotkeys: Vec::new(),
            #[cfg(feature = "native-host")]
            native_hosts: crate::NativeHosts::default(),
            #[cfg(all(windows, feature = "windows-win32", feature = "native-host"))]
            windows_hooks: None,
            icon_path: None,
            menu: None,
            content: None,
        }
    }

    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Places the initial window at a logical virtual-desktop position.
    pub fn position(mut self, position: WindowPosition) -> Self {
        self.position = Some(position);
        self
    }

    /// Places the initial window at logical `x`/`y` coordinates.
    pub fn position_at(self, x: crate::Dp, y: crate::Dp) -> Self {
        self.position(WindowPosition::new(x, y))
    }

    pub fn min_size(mut self, width: u32, height: u32) -> Self {
        self.min_width = Some(width);
        self.min_height = Some(height);
        self
    }

    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    /// Lets the application draw the title bar inside the client area while
    /// the platform keeps resize borders, snapping and the taskbar entry.
    /// Mark draggable parts of the View with `ViewNode::window_drag_region`
    /// and drive caption buttons through `AppCx::window_command`. Hosts
    /// without support keep their native title bar.
    #[cfg(feature = "window-chrome")]
    pub fn custom_title_bar(mut self, custom_title_bar: bool) -> Self {
        self.custom_title_bar = custom_title_bar;
        self
    }

    pub fn always_on_top(mut self, always_on_top: bool) -> Self {
        self.always_on_top = always_on_top;
        self
    }

    pub fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    pub fn icon_path(mut self, icon_path: impl Into<String>) -> Self {
        self.icon_path = Some(icon_path.into());
        self
    }

    pub fn menu(mut self, menu: MenuSpec) -> Self {
        self.menu = Some(menu);
        self
    }

    pub fn content(mut self, content: UiNode) -> Self {
        self.content = Some(content);
        self
    }

    pub fn native_options(&self) -> WindowNativeOptions {
        WindowNativeOptions {
            position: self.position,
            min_width: self.min_width,
            min_height: self.min_height,
            resizable: self.resizable,
            decorations: self.decorations,
            always_on_top: self.always_on_top,
            transparent: self.transparent,
        }
    }

    pub fn resolve_for(&self, capabilities: &HostCapabilities) -> WindowResolvedSpec {
        let mut effective = self.clone();
        if capabilities.window_resizing.status == CapabilityStatus::Unsupported {
            effective.resizable = true;
            effective.min_width = None;
            effective.min_height = None;
        }
        if capabilities.window_decorations.status == CapabilityStatus::Unsupported {
            effective.decorations = true;
        }
        if capabilities.window_always_on_top.status == CapabilityStatus::Unsupported {
            effective.always_on_top = false;
        }
        if capabilities.window_transparency.status == CapabilityStatus::Unsupported {
            effective.transparent = false;
        }
        if capabilities.window_positioning.status == CapabilityStatus::Unsupported {
            effective.position = None;
        }
        if capabilities.window_overlay.status == CapabilityStatus::Unsupported
            && effective.integration.layer.is_overlay()
        {
            effective.integration.layer = crate::WindowLayer::Normal;
        }

        WindowResolvedSpec {
            requested: self.clone(),
            effective,
            degraded_capabilities: self.degraded_capabilities(capabilities),
        }
    }

    pub fn degraded_capabilities(&self, capabilities: &HostCapabilities) -> Vec<String> {
        let mut degraded = Vec::new();
        push_if_degraded(
            &mut degraded,
            "windows",
            &capabilities.windows,
            "native window creation",
            "record the declaration and keep application state alive",
        );
        push_if_degraded(
            &mut degraded,
            "window_resizing",
            &capabilities.window_resizing,
            if self.resizable {
                "resizable window"
            } else {
                "fixed-size window"
            },
            if self.resizable {
                "host may create a fixed-size or custom window"
            } else {
                "host may leave the window resizable"
            },
        );
        push_if_degraded(
            &mut degraded,
            "window_decorations",
            &capabilities.window_decorations,
            if self.decorations {
                "native decorated window"
            } else {
                "undecorated window"
            },
            if self.decorations {
                "host may use custom chrome or an undecorated surface"
            } else {
                "host may keep native decorations"
            },
        );
        if self.always_on_top {
            push_if_degraded(
                &mut degraded,
                "window_always_on_top",
                &capabilities.window_always_on_top,
                "always-on-top window",
                "host may present a normal z-order window",
            );
        }
        if self.transparent {
            push_if_degraded(
                &mut degraded,
                "window_transparency",
                &capabilities.window_transparency,
                "transparent window",
                "host may present an opaque native window",
            );
        }
        if self.position.is_some() {
            push_if_degraded(
                &mut degraded,
                "window_positioning",
                &capabilities.window_positioning,
                "positioned window",
                "host may place the window at its default location",
            );
        }
        if self.integration.layer.is_overlay() {
            push_if_degraded(
                &mut degraded,
                "window_overlay",
                &capabilities.window_overlay,
                "overlay window",
                "host may present a normal window with reduced stacking and activation behavior",
            );
            push_if_degraded(
                &mut degraded,
                "window_metrics",
                &capabilities.window_metrics,
                "window bounds and work-area metrics",
                "host may not report monitor work-area changes",
            );
        }
        degraded
    }
}

fn push_if_degraded(
    degraded: &mut Vec<String>,
    capability: &'static str,
    support: &CapabilitySupport,
    requested: &'static str,
    fallback: &'static str,
) {
    if support.status == CapabilityStatus::Supported {
        return;
    }
    degraded.push(format!(
        "{capability}: requested {requested}; fallback {fallback}; {}",
        support.detail
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_position_round_trips_logical_coordinates() {
        let position = WindowPosition::new(crate::Dp::new(-12.5), crate::Dp::new(4.25));
        assert_eq!(position.x(), crate::Dp::new(-12.5));
        assert_eq!(position.y(), crate::Dp::new(4.25));
        assert_eq!(position.to_physical(crate::Dpi::standard()), (-13, 4));
        assert_eq!(WindowPosition::from_milli_dp(-12_500, 4_250), position);
    }
}

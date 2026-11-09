use super::{
    complete_native_smoke, DesktopNativeSmokeMetadata, DesktopNativeSmokeOutcome,
    DesktopRuntimeBackend, DesktopRuntimeRequest, DesktopSmokeRequest,
};
use crate::{
    DesktopCapabilities, DialogResponse, FileDialogSpec, HostCapabilities, NativeDialogSpec,
    NativeWindowSmokeRunReport, SaveFileDialogSpec, ZsuiError, ZsuiResult,
};

#[derive(Default)]
pub(super) struct Backend;

impl DesktopRuntimeBackend for Backend {
    #[cfg(test)]
    fn backend_name(&self) -> &'static str {
        "linux_direct"
    }

    fn validate_window_integration(
        &self,
        windows: &[crate::WindowSpec],
        plans: &[Option<crate::NativeDrawPlan>],
    ) -> ZsuiResult<()> {
        let overlay_supported = cfg!(all(feature = "desktop-overlay", feature = "linux-direct"));
        for window in windows {
            let integration = &window.integration;
            if integration.title_bar.is_some() {
                return Err(ZsuiError::unsupported(
                    "window.title_bar",
                    "the Linux direct host does not implement integrated system title bars",
                ));
            }
            if integration.renderer != crate::WindowRenderer::Gdi {
                return Err(ZsuiError::unsupported(
                    "window.renderer",
                    "the Linux direct host only provides its Cairo/Pango renderer",
                ));
            }
            if matches!(integration.layer, crate::WindowLayer::Desktop { .. }) {
                return Err(ZsuiError::unsupported(
                    "window.layer",
                    "the Linux direct host does not implement desktop-layer windows",
                ));
            }
            if integration.layer.is_overlay() && !overlay_supported {
                return Err(ZsuiError::unsupported(
                    "window.layer",
                    "enable desktop-overlay with linux-direct for X11/XWayland overlay windows",
                ));
            }
            if integration.surface != crate::WindowSurface::Opaque && !overlay_supported {
                return Err(ZsuiError::unsupported(
                    "window.surface",
                    "enable desktop-overlay with linux-direct for per-pixel-alpha windows",
                ));
            }
            #[cfg(feature = "native-host")]
            if !window.native_hosts.providers.is_empty() {
                return Err(ZsuiError::unsupported(
                    "native_host",
                    "the Linux direct host does not implement native child providers",
                ));
            }
        }
        #[cfg(feature = "native-host")]
        if plans
            .iter()
            .flatten()
            .any(|plan| !plan.native_hosts.is_empty())
        {
            return Err(ZsuiError::unsupported(
                "native_host",
                "the Linux direct host does not implement native child slots",
            ));
        }
        let _ = plans;
        Ok(())
    }

    fn run_event_loop(self, request: DesktopRuntimeRequest) -> ZsuiResult<()> {
        let _shell_runtimes = request.shell_runtimes;
        if !request.trays.is_empty() {
            return Err(ZsuiError::unsupported(
                "native_window_status_item",
                "the Linux direct status-item runtime is not connected to the unified event loop",
            ));
        }
        #[cfg(feature = "linux-wayland-layer")]
        if crate::linux_wayland_layer::should_use_layer_shell(&request.windows) {
            return crate::linux_wayland_layer::run_layer_shell_event_loop(
                &request.windows,
                &request.draw_plans,
                &request.view_runtimes,
            )
            .map(|_| ());
        }
        crate::linux_direct::run_linux_direct_native_window_event_loop(
            &request.windows,
            &request.draw_plans,
            &request.view_runtimes,
            None,
            None,
            &[],
            None,
        )
        .map(|_| ())
    }

    fn run_smoke_event_loop(
        self,
        request: DesktopSmokeRequest,
    ) -> ZsuiResult<NativeWindowSmokeRunReport> {
        if request.windows.is_empty() {
            return Ok(NativeWindowSmokeRunReport::empty(request.options));
        }
        let run = crate::linux_direct::run_linux_direct_native_window_event_loop(
            &request.windows,
            &request.draw_plans,
            std::slice::from_ref(&request.view_runtime),
            Some(request.options.auto_close_after_ms),
            request
                .options
                .screenshot_file
                .as_deref()
                .map(std::path::Path::new),
            &request.options.native_view_inputs,
            request.options.native_window_resize,
        )?;
        complete_native_smoke(
            request,
            DesktopNativeSmokeOutcome {
                created_window_count: run.created_window_count,
                proof_input_reports: run.proof_input_reports,
                native_view_capture: run.native_view_capture,
                native_overlay: run.native_overlay,
                native_window_resize: run.native_window_resize,
                native_window_resize_error: run.native_window_resize_error,
                menu_command_routed: run.menu_command_routed,
                menu_surface_created: run.menu_surface_created,
                menu_surface_height: run.menu_surface_height,
                menu_surface_open_at_capture: run.menu_surface_open_at_capture,
                status_item_created: false,
                status_menu_native_command_count: 0,
                status_menu_command_routed: false,
                status_menu_popup_created: false,
                status_menu_popup_destroyed: false,
                process_memory: run.process_memory,
                accessibility_backend: run
                    .accessibility_bridge_created
                    .then_some("accesskit_atspi"),
                accessibility_node_count: run.accessibility_node_count,
                accessibility_action_count: run.accessibility_action_count,
            },
            DesktopNativeSmokeMetadata {
                proof_backend: "linux_direct",
                screenshot_backend: crate::linux_direct::capture_backend_name(),
                missing_capture_error:
                    "the Linux direct event loop exited before the final surface capture",
            },
        )
    }

    fn scaffold_capabilities(&self) -> HostCapabilities {
        HostCapabilities::linux_scaffold()
    }

    fn native_host_capabilities(&self) -> HostCapabilities {
        HostCapabilities::linux_native_window_host()
    }

    fn desktop_capabilities(&self) -> DesktopCapabilities {
        DesktopCapabilities::linux_direct_current()
    }

    fn native_proof_backend_name(&self) -> &'static str {
        "linux-direct"
    }

    fn native_proof_typography(&self, typography_scale: f32) -> crate::NativeTypographyProfile {
        crate::NativeTypographyProfile::fallback(
            crate::ZsPlatformStyle::current(),
            typography_scale,
        )
    }

    fn capture_process_memory(
        &self,
        sample_point: &'static str,
    ) -> Option<crate::NativeProofProcessMemoryEvidence> {
        super::process_memory::capture_linux(sample_point)
    }

    #[cfg(feature = "clipboard")]
    fn read_clipboard(&mut self) -> ZsuiResult<Option<crate::ClipboardData>> {
        crate::linux_direct::linux_direct_read_clipboard()
    }

    #[cfg(feature = "clipboard")]
    fn write_clipboard(&mut self, data: &crate::ClipboardData) -> ZsuiResult<()> {
        crate::linux_direct::linux_direct_write_clipboard(data)
    }

    fn open_file_dialog(
        &mut self,
        spec: &FileDialogSpec,
    ) -> Option<ZsuiResult<Option<Vec<std::path::PathBuf>>>> {
        Some(crate::linux_direct::linux_direct_open_file_dialog(spec))
    }

    fn save_file_dialog(
        &mut self,
        spec: &SaveFileDialogSpec,
    ) -> ZsuiResult<Option<std::path::PathBuf>> {
        crate::linux_direct::linux_direct_save_file_dialog(spec)
    }

    fn pick_directory(
        &mut self,
        spec: &crate::DirectoryDialogSpec,
    ) -> ZsuiResult<Option<std::path::PathBuf>> {
        crate::linux_direct::linux_direct_pick_directory_dialog(spec)
    }

    fn show_native_dialog(&mut self, spec: &NativeDialogSpec) -> ZsuiResult<DialogResponse> {
        crate::linux_direct::linux_direct_show_native_dialog(spec)
    }
}

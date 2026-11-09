//! Overlay-only Wayland layer-shell host.
//!
//! This backend is intentionally narrow: it presents `WindowLayer::Overlay`
//! windows on compositors that expose `zwlr_layer_shell_v1`. Normal windows,
//! IME, menus, dialogs and accessibility stay on the existing winit/X11 path.

use std::time::Instant;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop,
        calloop_wayland_source::WaylandSource,
        client::{
            globals::{registry_queue_init, GlobalListContents},
            protocol::{wl_output, wl_pointer, wl_registry, wl_seat, wl_shm, wl_surface},
            Connection, Dispatch, QueueHandle,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use winit::window::Theme as WinitTheme;

use crate::{
    Dp, NativeDrawPlan, NativeWindowEvent, Point, Rect, WindowLayer, WindowMetrics, WindowPosition,
    WindowSpec, ZsPointerButton,
};

struct Probe;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Probe {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl LayerShellHandler for Probe {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {}

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
    }
}

delegate_layer!(Probe);

/// True when every requested window is an overlay and this compositor exposes
/// the wlr layer-shell protocol.
pub(crate) fn should_use_layer_shell(windows: &[WindowSpec]) -> bool {
    if windows.is_empty()
        || !windows
            .iter()
            .all(|window| window.integration.layer.is_overlay())
    {
        return false;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return false;
    }
    let Ok(connection) = Connection::connect_to_env() else {
        return false;
    };
    let Ok((globals, event_queue)) = registry_queue_init::<Probe>(&connection) else {
        return false;
    };
    let qh = event_queue.handle();
    LayerShell::bind(&globals, &qh).is_ok()
}

pub(crate) fn run_layer_shell_event_loop(
    windows: &[WindowSpec],
    draw_plans: &[Option<NativeDrawPlan>],
    view_runtimes: &[crate::native::NativeViewInputRuntime],
) -> crate::ZsuiResult<()> {
    if windows.is_empty() {
        return Ok(());
    }
    if !windows
        .iter()
        .all(|window| window.integration.layer.is_overlay())
    {
        return Err(crate::ZsuiError::unsupported(
            "linux_wayland_layer",
            "the layer-shell host only accepts overlay windows",
        ));
    }
    let connection = Connection::connect_to_env().map_err(|error| {
        crate::ZsuiError::host(
            "linux_wayland_layer",
            format!("could not connect to Wayland: {error}"),
        )
    })?;
    let (globals, event_queue) =
        registry_queue_init::<LayerHost>(&connection).map_err(|error| {
            crate::ZsuiError::host(
                "linux_wayland_layer",
                format!("could not enumerate Wayland globals: {error}"),
            )
        })?;
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).map_err(|error| {
        crate::ZsuiError::host(
            "linux_wayland_layer",
            format!("wl_compositor is unavailable: {error}"),
        )
    })?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|error| {
        crate::ZsuiError::host(
            "linux_wayland_layer",
            format!("zwlr_layer_shell_v1 is unavailable: {error}"),
        )
    })?;
    let shm = Shm::bind(&globals, &qh).map_err(|error| {
        crate::ZsuiError::host(
            "linux_wayland_layer",
            format!("wl_shm is unavailable: {error}"),
        )
    })?;

    let mut host = LayerHost {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        layer_shell,
        windows: Vec::new(),
        pointer: None,
        exit: false,
        started_at: Instant::now(),
    };

    for (index, spec) in windows.iter().enumerate() {
        let surface = host.compositor.create_surface(&qh);
        let layer = host.layer_shell.create_layer_surface(
            &qh,
            surface,
            Layer::Top,
            Some("zsui-overlay"),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        let position = spec.position.unwrap_or(WindowPosition::from_milli_dp(0, 0));
        let margin_x = position.x().0.round().max(0.0) as i32;
        let margin_y = position.y().0.round().max(0.0) as i32;
        layer.set_margin(margin_y, 0, 0, margin_x);
        layer.set_size(spec.width.max(1), spec.height.max(1));
        layer.set_exclusive_zone(0);
        let activate_on_click = matches!(
            spec.integration.layer,
            WindowLayer::Overlay {
                activate_on_click: true
            }
        );
        layer.set_keyboard_interactivity(if activate_on_click {
            KeyboardInteractivity::OnDemand
        } else {
            KeyboardInteractivity::None
        });
        layer.commit();
        let mut runtime = view_runtimes.get(index).cloned().unwrap_or_default();
        if let Some(plan) = draw_plans.get(index).and_then(Option::as_ref) {
            runtime.set_surface(
                Rect {
                    x: 0,
                    y: 0,
                    width: spec.width as i32,
                    height: spec.height as i32,
                },
                crate::Dpi::new(96.0),
            );
            let _ = plan;
        }
        host.windows.push(LayerWindow {
            spec: spec.clone(),
            layer,
            runtime,
            plan: draw_plans
                .get(index)
                .and_then(Option::clone)
                .unwrap_or_default(),
            renderer: crate::linux_direct::LinuxDirectOverlayRenderer::new(),
            pool: SlotPool::new(
                (spec.width.max(1) as usize)
                    .saturating_mul(spec.height.max(1) as usize)
                    .saturating_mul(4),
                &host.shm,
            )
            .map_err(|error| {
                crate::ZsuiError::host(
                    "linux_wayland_layer",
                    format!("could not create a Wayland buffer pool: {error}"),
                )
            })?,
            size: (spec.width.max(1), spec.height.max(1)),
            configured: false,
        });
    }

    let mut event_loop: calloop::EventLoop<LayerHost> = calloop::EventLoop::try_new()
        .map_err(|error| crate::ZsuiError::host("linux_wayland_layer", error.to_string()))?;
    let loop_handle = event_loop.handle();
    WaylandSource::new(connection, event_queue)
        .insert(loop_handle.clone())
        .map_err(|error| crate::ZsuiError::host("linux_wayland_layer", error.to_string()))?;
    let (ping, ping_source) = calloop::ping::make_ping()
        .map_err(|error| crate::ZsuiError::host("linux_wayland_layer", error.to_string()))?;
    loop_handle
        .insert_source(ping_source, |_, _, host: &mut LayerHost| {
            host.redraw_all(&qh);
        })
        .map_err(|error| crate::ZsuiError::host("linux_wayland_layer", error.to_string()))?;
    for window in &mut host.windows {
        let ping = ping.clone();
        window.runtime.bind_invalidation_target(move || {
            ping.ping();
        });
    }

    while !host.exit {
        event_loop
            .dispatch(None, &mut host)
            .map_err(|error| crate::ZsuiError::host("linux_wayland_layer", error.to_string()))?;
    }
    Ok(())
}

struct LayerHost {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    windows: Vec<LayerWindow>,
    pointer: Option<wl_pointer::WlPointer>,
    exit: bool,
    #[allow(dead_code)]
    started_at: Instant,
}

struct LayerWindow {
    spec: WindowSpec,
    layer: LayerSurface,
    runtime: crate::native::NativeViewInputRuntime,
    plan: NativeDrawPlan,
    renderer: crate::linux_direct::LinuxDirectOverlayRenderer,
    pool: SlotPool,
    size: (u32, u32),
    configured: bool,
}

impl LayerHost {
    fn redraw_all(&mut self, qh: &QueueHandle<Self>) {
        for index in 0..self.windows.len() {
            self.draw_window(index, qh);
        }
    }

    fn draw_window(&mut self, index: usize, qh: &QueueHandle<Self>) {
        let region = Region::new(&self.compositor).ok();
        let Some(window) = self.windows.get_mut(index) else {
            return;
        };
        let (width, height) = window.size;
        if width == 0 || height == 0 {
            return;
        }
        let stride = match i32::try_from(width.saturating_mul(4)) {
            Ok(stride) => stride,
            Err(_) => return,
        };
        let (buffer, canvas) = match window.pool.create_buffer(
            width as i32,
            height as i32,
            stride,
            wl_shm::Format::Argb8888,
        ) {
            Ok(buffer) => buffer,
            Err(error) => {
                eprintln!("zsui layer-shell buffer allocation failed: {error}");
                return;
            }
        };
        let mut frame = vec![0u32; width as usize * height as usize];
        if let Err(error) = window.renderer.render(
            &mut frame,
            &window.plan,
            width,
            height,
            1.0,
            WinitTheme::Light,
        ) {
            eprintln!("zsui layer-shell render failed: {error}");
            return;
        }
        for (chunk, pixel) in canvas.chunks_exact_mut(4).zip(frame.iter()) {
            chunk.copy_from_slice(&pixel.to_le_bytes());
        }
        if let Some(region) = region {
            for y in 0..height {
                let row = &frame[y as usize * width as usize..(y as usize + 1) * width as usize];
                let mut x = 0usize;
                while x < row.len() {
                    if row[x] >> 24 == 0 {
                        x += 1;
                        continue;
                    }
                    let start = x;
                    while x < row.len() && row[x] >> 24 != 0 {
                        x += 1;
                    }
                    region.add(start as i32, y as i32, (x - start) as i32, 1);
                }
            }
            window
                .layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        let surface = window.layer.wl_surface();
        surface.damage_buffer(0, 0, width as i32, height as i32);
        if let Err(error) = buffer.attach_to(surface) {
            eprintln!("zsui layer-shell buffer attach failed: {error}");
            return;
        }
        window.layer.commit();
        let _ = qh;
    }

    fn apply_report(
        &mut self,
        index: usize,
        report: crate::native::NativeViewInputDispatchReport,
        qh: &QueueHandle<Self>,
    ) {
        if let Some(position) = report.window_position_request {
            if let Some(window) = self.windows.get_mut(index) {
                let margin_x = position.x().0.round().max(0.0) as i32;
                let margin_y = position.y().0.round().max(0.0) as i32;
                window.layer.set_margin(margin_y, 0, 0, margin_x);
                window.spec.position = Some(position);
                window.layer.commit();
            }
        }
        if let Some(plan) = report.redraw_plan {
            if let Some(window) = self.windows.get_mut(index) {
                window.plan = plan;
            }
            self.draw_window(index, qh);
        }
        if report.quit_requested {
            self.exit = true;
        }
        for error in report.errors {
            eprintln!("zsui layer-shell input error: {error}");
        }
    }

    fn window_index_for_surface(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.windows
            .iter()
            .position(|window| window.layer.wl_surface() == surface)
    }

    fn metrics(&self, window: &LayerWindow) -> WindowMetrics {
        let position = window.spec.position;
        let margin = |value: Dp| value.0.round().max(0.0) as i32;
        let output = self.output_state.outputs().next();
        let info = output
            .as_ref()
            .and_then(|output| self.output_state.info(output));
        let (work_width, work_height, dpi) = info
            .as_ref()
            .and_then(|info| info.logical_size)
            .map(|(width, height)| {
                (
                    width,
                    height,
                    info.as_ref()
                        .map_or(96, |info| (info.scale_factor.max(1) as u32) * 96),
                )
            })
            .unwrap_or((window.size.0 as i32, window.size.1 as i32, 96));
        WindowMetrics {
            bounds: Rect {
                x: position.map_or(0, |position| margin(position.x())),
                y: position.map_or(0, |position| margin(position.y())),
                width: window.size.0 as i32,
                height: window.size.1 as i32,
            },
            work_area: Rect {
                x: 0,
                y: 0,
                width: work_width,
                height: work_height,
            },
            dpi,
        }
    }
}

impl CompositorHandler for LayerHost {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for LayerHost {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let Some(index) = self
            .windows
            .iter()
            .position(|window| window.layer.wl_surface() == layer.wl_surface())
        else {
            return;
        };
        let (width, height) = if configure.new_size.0 == 0 || configure.new_size.1 == 0 {
            let spec = &self.windows[index].spec;
            (spec.width.max(1), spec.height.max(1))
        } else {
            configure.new_size
        };
        self.windows[index].size = (width.max(1), height.max(1));
        self.windows[index].configured = true;
        let metrics = self.metrics(&self.windows[index]);
        let report = self.windows[index]
            .runtime
            .dispatch_native_window_event(NativeWindowEvent::MetricsChanged(metrics));
        self.apply_report(index, report, qh);
        self.draw_window(index, qh);
    }
}

impl OutputHandler for LayerHost {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl SeatHandler for LayerHost {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            if let Ok(pointer) = self.seat_state.get_pointer(qh, &seat) {
                self.pointer = Some(pointer);
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl PointerHandler for LayerHost {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            let Some(index) = self.window_index_for_surface(&event.surface) else {
                continue;
            };
            let point = Point {
                x: event.position.0.round() as i32,
                y: event.position.1.round() as i32,
            };
            let modifiers = crate::ZsPointerModifiers::default();
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let report = self.windows[index]
                        .runtime
                        .dispatch_pointer_move_with_modifiers(point, modifiers);
                    self.apply_report(index, report, qh);
                }
                PointerEventKind::Leave { .. } => {
                    let report = self.windows[index].runtime.dispatch_pointer_leave();
                    self.apply_report(index, report, qh);
                }
                PointerEventKind::Press { button, .. } => {
                    if let Some(button) = wayland_pointer_button(button) {
                        let report = self.windows[index]
                            .runtime
                            .dispatch_pointer_down_with_button(point, button, modifiers);
                        self.apply_report(index, report, qh);
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    if let Some(button) = wayland_pointer_button(button) {
                        let report = self.windows[index]
                            .runtime
                            .dispatch_pointer_up_with_button(point, button, modifiers);
                        self.apply_report(index, report, qh);
                    }
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    let delta = vertical.absolute
                        + horizontal.absolute
                        + f64::from(vertical.discrete) * 48.0
                        + f64::from(horizontal.discrete) * 48.0;
                    if delta.abs() > f64::EPSILON {
                        let report = self.windows[index]
                            .runtime
                            .dispatch_pointer_scroll(point, Dp::new(delta as f32));
                        self.apply_report(index, report, qh);
                    }
                }
            }
        }
    }
}

impl ShmHandler for LayerHost {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for LayerHost {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(LayerHost);
delegate_layer!(LayerHost);
delegate_output!(LayerHost);
delegate_pointer!(LayerHost);
delegate_registry!(LayerHost);
delegate_seat!(LayerHost);
delegate_shm!(LayerHost);

fn wayland_pointer_button(button: u32) -> Option<ZsPointerButton> {
    match button {
        0x110 => Some(ZsPointerButton::Primary),
        0x111 => Some(ZsPointerButton::Secondary),
        0x112 => Some(ZsPointerButton::Middle),
        value => u16::try_from(value).ok().map(ZsPointerButton::Auxiliary),
    }
}

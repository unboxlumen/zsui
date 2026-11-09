//! X11/XWayland per-pixel-alpha overlay presentation.
//!
//! The normal Linux Direct presenter uses softbuffer's 0RGB surface. Overlay
//! windows instead need an ARGB buffer, an X11 depth-32 visual, a topmost
//! override-redirect window, and an input shape derived from the same alpha
//! mask. This module owns only that platform presentation layer.

use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection;
use x11rb::protocol::shape;
use x11rb::protocol::xproto::{
    self, AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateGCAux, ImageFormat, ImageOrder,
    PropMode, Rectangle, StackMode,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub(crate) struct LinuxX11OverlayPresenter {
    connection: RustConnection,
    window: u32,
    gc: u32,
    width: u32,
    height: u32,
    frame: Vec<u32>,
    shape_supported: bool,
    depth: u32,
    hit_test: &'static str,
    input_rectangle_count: usize,
    alpha_pixel_count: usize,
    opaque_pixel_count: usize,
}

impl LinuxX11OverlayPresenter {
    pub(crate) fn new(
        window: &winit::window::Window,
        activate_on_click: bool,
        hit_test: &'static str,
    ) -> Result<Self, String> {
        let window_id = x11_window_id(window)?;
        let (connection, screen_num) = x11rb::connect(None)
            .map_err(|error| format!("could not connect to the X11 display: {error}"))?;
        let screen = &connection.setup().roots[screen_num];
        let root = screen.root;
        let geometry = connection
            .get_geometry(window_id)
            .map_err(|error| format!("could not query the X11 overlay geometry: {error}"))?
            .reply()
            .map_err(|error| format!("could not query the X11 overlay geometry: {error}"))?;
        if geometry.depth != 32 {
            return Err(format!(
                "the X11 overlay requires a 32-bit ARGB visual; the window depth is {}",
                geometry.depth
            ));
        }
        let gc = connection
            .generate_id()
            .map_err(|error| format!("could not allocate an X11 overlay GC: {error}"))?;
        connection
            .create_gc(gc, window_id, &CreateGCAux::default())
            .map_err(|error| format!("could not create an X11 overlay GC: {error}"))?;
        let shape_supported = shape::query_version(&connection)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some();
        let mut presenter = Self {
            connection,
            window: window_id,
            gc,
            width: geometry.width.max(1) as u32,
            height: geometry.height.max(1) as u32,
            frame: Vec::new(),
            shape_supported,
            depth: geometry.depth as u32,
            hit_test,
            input_rectangle_count: 0,
            alpha_pixel_count: 0,
            opaque_pixel_count: 0,
        };
        presenter.resize(presenter.width, presenter.height);
        if activate_on_click {
            presenter.apply_managed_overlay_hints(root);
        }
        Ok(presenter)
    }

    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        let len = self.width as usize * self.height as usize;
        if self.frame.len() != len {
            self.frame.resize(len, 0);
        }
    }

    pub(crate) fn frame_mut(&mut self) -> &mut [u32] {
        &mut self.frame
    }

    pub(crate) fn frame(&self) -> &[u32] {
        &self.frame
    }

    pub(crate) fn depth(&self) -> u32 {
        self.depth
    }

    pub(crate) fn hit_test(&self) -> &'static str {
        self.hit_test
    }

    pub(crate) fn input_rectangle_count(&self) -> usize {
        self.input_rectangle_count
    }

    pub(crate) fn alpha_pixel_count(&self) -> usize {
        self.alpha_pixel_count
    }

    pub(crate) fn opaque_pixel_count(&self) -> usize {
        self.opaque_pixel_count
    }

    pub(crate) fn present(&mut self) -> Result<(), String> {
        self.alpha_pixel_count = self.frame.iter().filter(|pixel| **pixel >> 24 != 0).count();
        self.opaque_pixel_count = self
            .frame
            .iter()
            .filter(|pixel| **pixel >> 24 == 0xff)
            .count();
        let width =
            u16::try_from(self.width).map_err(|_| "X11 overlay width exceeds u16".to_string())?;
        let height =
            u16::try_from(self.height).map_err(|_| "X11 overlay height exceeds u16".to_string())?;
        let lsb_first = matches!(
            self.connection.setup().image_byte_order,
            ImageOrder::LSB_FIRST
        );
        let mut bytes = Vec::with_capacity(self.frame.len() * 4);
        for pixel in &self.frame {
            let a = (pixel >> 24) as u8;
            let r = (pixel >> 16) as u8;
            let g = (pixel >> 8) as u8;
            let b = *pixel as u8;
            if lsb_first {
                bytes.extend_from_slice(&[b, g, r, a]);
            } else {
                bytes.extend_from_slice(&[a, r, g, b]);
            }
        }
        self.connection
            .put_image(
                ImageFormat::Z_PIXMAP,
                self.window,
                self.gc,
                width,
                height,
                0,
                0,
                0,
                32,
                &bytes,
            )
            .map_err(|error| format!("could not upload the X11 overlay frame: {error}"))?;
        self.connection
            .configure_window(
                self.window,
                &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            )
            .map_err(|error| format!("could not raise the X11 overlay window: {error}"))?;
        self.update_input_shape()?;
        self.connection
            .flush()
            .map_err(|error| format!("could not flush the X11 overlay connection: {error}"))?;
        Ok(())
    }

    fn update_input_shape(&mut self) -> Result<(), String> {
        if !self.shape_supported {
            self.input_rectangle_count = 0;
            return Ok(());
        }
        let mut rectangles = Vec::new();
        for y in 0..self.height {
            let start = y as usize * self.width as usize;
            let row = &self.frame[start..start + self.width as usize];
            let mut x = 0usize;
            while x < row.len() {
                if row[x] >> 24 == 0 {
                    x += 1;
                    continue;
                }
                let run_start = x;
                while x < row.len() && row[x] >> 24 != 0 {
                    x += 1;
                }
                let run_end = x;
                let left = i16::try_from(run_start)
                    .map_err(|_| "X11 overlay input shape x exceeds i16".to_string())?;
                let top = i16::try_from(y)
                    .map_err(|_| "X11 overlay input shape y exceeds i16".to_string())?;
                let width = u16::try_from(run_end - run_start)
                    .map_err(|_| "X11 overlay input shape width exceeds u16".to_string())?;
                rectangles.push(Rectangle {
                    x: left,
                    y: top,
                    width,
                    height: 1,
                });
            }
        }
        self.input_rectangle_count = rectangles.len();
        shape::rectangles(
            &self.connection,
            shape::SO::SET,
            shape::SK::INPUT,
            xproto::ClipOrdering::UNSORTED,
            self.window,
            0,
            0,
            &rectangles,
        )
        .map_err(|error| format!("could not update the X11 overlay input shape: {error}"))?;
        Ok(())
    }

    fn apply_managed_overlay_hints(&mut self, root: u32) {
        let atoms = [
            "_NET_WM_WINDOW_TYPE",
            "_NET_WM_WINDOW_TYPE_UTILITY",
            "_NET_WM_STATE",
            "_NET_WM_STATE_ABOVE",
            "_NET_WM_STATE_SKIP_TASKBAR",
            "_NET_WM_STATE_SKIP_PAGER",
        ];
        let mut resolved = [0u32; 6];
        for (slot, name) in resolved.iter_mut().zip(atoms) {
            let Ok(cookie) = self.connection.intern_atom(false, name.as_bytes()) else {
                return;
            };
            let Ok(reply) = cookie.reply() else {
                return;
            };
            *slot = reply.atom;
        }
        let [window_type, utility, state, above, skip_taskbar, skip_pager] = resolved;
        let _ = root;
        let _ = self.connection.change_property32(
            PropMode::REPLACE,
            self.window,
            window_type,
            AtomEnum::ATOM,
            &[utility],
        );
        let _ = self.connection.change_property32(
            PropMode::REPLACE,
            self.window,
            state,
            AtomEnum::ATOM,
            &[above, skip_taskbar, skip_pager],
        );
    }
}

impl Drop for LinuxX11OverlayPresenter {
    fn drop(&mut self) {
        let _ = self.connection.free_gc(self.gc);
        let _ = self.connection.flush();
    }
}

fn x11_window_id(window: &winit::window::Window) -> Result<u32, String> {
    let handle = window
        .window_handle()
        .map_err(|error| format!("could not access the X11 overlay window handle: {error}"))?;
    match handle.as_raw() {
        RawWindowHandle::Xlib(handle) => {
            u32::try_from(handle.window).map_err(|_| "the Xlib window id exceeds u32".to_string())
        }
        RawWindowHandle::Xcb(handle) => Ok(handle.window.get()),
        _ => Err("the overlay window is not an X11 window".to_string()),
    }
}

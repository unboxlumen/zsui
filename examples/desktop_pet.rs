#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! Minimal desktop-pet overlay.
//!
//! The window is borderless, transparent, always-on-top and click-through
//! outside the painted pet. Click the pet to move it. Run on Linux/X11 or
//! XWayland with:
//!
//! `cargo run --example desktop_pet --features "window,canvas,desktop-overlay"`

use zsui::*;

const PET: WidgetId = WidgetId::new(1);

#[derive(Clone, Debug)]
enum Message {
    Move,
    Ignore,
    Metrics(WindowMetrics),
}

struct Pet {
    position: WindowPosition,
    work_area: Option<Rect>,
}

impl Pet {
    fn new() -> Self {
        Self {
            position: WindowPosition::new(Dp::new(360.0), Dp::new(240.0)),
            work_area: None,
        }
    }
}

fn view(_state: &Pet) -> ViewNode<Message> {
    let scene = ZsCanvasScene::new()
        .with(ZsCanvasPrimitive::round_fill(
            ZsCanvasRect::new(Dp::new(10.0), Dp::new(10.0), Dp::new(200.0), Dp::new(200.0)),
            NativeDrawFill::role(ColorRole::Accent),
            Dp::new(48.0),
        ))
        .with(ZsCanvasPrimitive::round_fill(
            ZsCanvasRect::new(Dp::new(58.0), Dp::new(66.0), Dp::new(28.0), Dp::new(28.0)),
            NativeDrawFill::role(ColorRole::Surface),
            Dp::new(14.0),
        ))
        .with(ZsCanvasPrimitive::round_fill(
            ZsCanvasRect::new(Dp::new(134.0), Dp::new(66.0), Dp::new(28.0), Dp::new(28.0)),
            NativeDrawFill::role(ColorRole::Surface),
            Dp::new(14.0),
        ));
    canvas(scene)
        .id(PET)
        .width(Dp::new(220.0))
        .height(Dp::new(220.0))
        .on_click(Message::Move)
        .on_native_window_event(|event| match event {
            NativeWindowEvent::MetricsChanged(metrics) => Message::Metrics(metrics),
            _ => Message::Ignore,
        })
}

fn update(state: &mut Pet, message: Message, cx: &mut AppCx) {
    match message {
        Message::Move => {
            let step = 24.0;
            let mut x = state.position.x().0 + step;
            let mut y = state.position.y().0 + step;
            if let Some(work_area) = state.work_area {
                let max_x = work_area.x as f32 + (work_area.width as f32 - 220.0).max(0.0);
                let max_y = work_area.y as f32 + (work_area.height as f32 - 220.0).max(0.0);
                if x > max_x {
                    x = work_area.x as f32;
                }
                if y > max_y {
                    y = work_area.y as f32;
                }
            }
            state.position = WindowPosition::new(Dp::new(x), Dp::new(y));
            cx.move_window_to(state.position);
        }
        Message::Metrics(metrics) => {
            let scale = metrics.scale_factor();
            state.work_area = Some(Rect {
                x: (metrics.work_area.x as f32 / scale).round() as i32,
                y: (metrics.work_area.y as f32 / scale).round() as i32,
                width: (metrics.work_area.width as f32 / scale).round() as i32,
                height: (metrics.work_area.height as f32 / scale).round() as i32,
            });
        }
        Message::Ignore => {}
    }
}

fn main() -> ZsuiResult<()> {
    let builder = native_window("ZSUI Desktop Pet")
        .size(220, 220)
        .position_at(Dp::new(360.0), Dp::new(240.0))
        .decorations(false)
        .surface(WindowSurface::PerPixelAlpha {
            hit_test: AlphaHitTest::TransparentPixels,
        })
        .layer(WindowLayer::Overlay {
            activate_on_click: true,
        })
        .stateful_view(Pet::new(), view, update);
    if let Some(directory) = smoke_directory() {
        #[cfg(feature = "native-smoke")]
        {
            std::fs::create_dir_all(&directory).map_err(|error| {
                ZsuiError::host(
                    "desktop_pet_smoke",
                    format!("could not create proof dir: {error}"),
                )
            })?;
            let png = directory.join("desktop-pet.png");
            let report = builder.run_smoke(
                NativeWindowSmokeRunOptions::new(1500)
                    .screenshot_file(png.to_string_lossy().to_string())
                    .require_screenshot(true),
            )?;
            if let Some(overlay) = &report.native_overlay {
                println!(
                    "overlay proof: backend={} depth={:?} input_rects={} alpha_pixels={} opaque_pixels={}",
                    overlay.backend,
                    overlay.window_depth,
                    overlay.input_rectangle_count,
                    overlay.alpha_pixel_count,
                    overlay.opaque_pixel_count
                );
            }
            std::fs::write(
                directory.join("desktop-pet.json"),
                serde_json::to_vec_pretty(&report).map_err(|error| {
                    ZsuiError::host(
                        "desktop_pet_smoke",
                        format!("could not serialize proof report: {error}"),
                    )
                })?,
            )
            .map_err(|error| {
                ZsuiError::host(
                    "desktop_pet_smoke",
                    format!("could not write proof report: {error}"),
                )
            })?;
        }
        #[cfg(not(feature = "native-smoke"))]
        eprintln!("--smoke requires the native-smoke feature");
        return Ok(());
    }
    builder.run()?;
    Ok(())
}

fn smoke_directory() -> Option<std::path::PathBuf> {
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--smoke" {
            return Some(
                arguments
                    .next()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from("target/desktop-pet-proof")),
            );
        }
    }
    None
}

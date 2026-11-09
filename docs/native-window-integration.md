# Native window integration

ZSUI combines retained Rust Views with window-owned native services. Applications
declare native child identities, layout, appearance and typed actions; the Win32
backend owns HWNDs, COM apartments, message dispatch and resource cleanup.

## Features

| Feature | Capability |
| --- | --- |
| `native-host` | Stable native child slots, layout clipping, focus and provider lifetime |
| `native-window-events` | Typed caption, appearance, desktop and error notifications |
| `window-chrome` | Custom drag regions, window commands and integrated system captions |
| `windows-shell` | Explorer views, namespace trees, native menu bridges and OLE file transfers |
| `clipboard-files` | File clipboard read/write through the shared Shell data-object adapter |
| `window-surface` | Premultiplied per-pixel alpha and configurable pointer pass-through |
| `desktop-layer` | Non-activating windows above desktop surfaces and below normal applications |
| `desktop-overlay` | Topmost overlay windows, logical placement and `NativeWindowEvent::MetricsChanged` |
| `linux-wayland-layer` | Overlay-only native Wayland layers on compositors exposing `zwlr_layer_shell_v1` |
| `windows-direct2d` | Explicit software or hardware Direct2D rasterization |
| `dark-mode` | Appearance propagation into native controls and Shell views |
| `hotkey` | Window-owned system-wide shortcuts with conflict reporting |

The default renderer remains buffered GDI/GDI+. Enabling `windows-direct2d` does
not switch ordinary windows automatically. Per-pixel surfaces select a software
Direct2D target by default. Win32 owns the full integration policy described
below. AppKit and Linux Direct implement the overlay subset described in
[Overlay windows](#overlay-windows); GTK4 and `linux-direct-lite` return an
explicit unsupported-capability error for overlays.

## Native child slots

```rust,no_run
# #[cfg(all(windows, feature = "native-host", feature = "label", feature = "window"))]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use zsui::prelude::*;

let editor = NativeHostId::new(10);
let providers = NativeHosts::new().edit(editor, "", false)?;
window("Native control")
    .size(640, 360)
    .native_hosts(providers)
    .view(column([
        text::<()>("Name"),
        native_host(editor).height(Dp::new(34.0)),
    ]).padding(Dp::new(16.0)))
    .run()?;
# Ok(())
# }
# #[cfg(not(all(windows, feature = "native-host", feature = "label", feature = "window")))]
# fn main() {}
```

`NativeHosts::explorer` and `NativeHosts::namespace_tree` register Shell folder
providers behind `windows-shell`. A provider is created when its ID first appears
in the View and is destroyed when the declaration removes that ID or the window
is destroyed. Compatible rebuilds preserve the native control and its state.
Duplicate IDs and missing providers are errors.

Ordinary View dimensions and flex layout determine the final physical bounds.
Ancestor clips and later popup surfaces are subtracted from native child regions.
The parent retains `WS_CLIPCHILDREN`, so buffered presentation does not overwrite
child pixels. Native child slots require an opaque parent surface.

Tab and F6 use the shared focus chain after the child and registered filters have
declined a message. `native_focusable(false)` removes a slot from traversal. File
drop regions do not create additional Tab stops or cover caption drag regions.

Third-party control adapters implement `WindowsNativeChild` and register factories
with `windows_win32_host::interop::WindowsWindowHooks`. Factories execute on the
owning UI thread. `NativeParent` is borrowed during attachment, and
`NativeChildWindow` owns child destruction and protects against HWND reuse. Raw
handles stay within this explicit backend adapter boundary.

## Message filters

`WindowsWindowHooks::message_filter` creates a separate `NativeMessageFilter` for
each window. A focused child's accelerator handler runs first, followed by
application filters in registration order. Returning `true` consumes the message
before `TranslateMessage` and `DispatchMessage`; an unhandled traversal key then
passes through the shared View focus route. Filters are removed with their window,
including destruction that occurs inside a filter callback.

## Integrated title bars

`WindowBuilder::integrated_title_bar(height, backdrop)` uses the native resize
frame and system caption buttons. `WindowBackdrop::Mica` requests the system
backdrop and falls back to a solid themed surface when unavailable. Caption hit
testing passes through DWM and retains the native maximize-button hit code used by
Snap Layouts. Closing follows the ordinary close-veto policy.

Place View content in the remaining title area and mark its noninteractive region
with `window_drag_region()`. Handle `NativeWindowEvent::CaptionChanged` and reserve
`CaptionMetrics::reserved_width()` at the trailing edge. Geometry notifications
follow creation, size and scale changes. Integrated captions require decorated,
opaque windows; the existing application-drawn `custom_title_bar` path remains
available.

## Transparent and desktop surfaces

Use `decorations(false)` with
`WindowSurface::PerPixelAlpha { hit_test: AlphaHitTest::TransparentPixels }`.
The host presents a premultiplied surface with `UpdateLayeredWindow`, including
the initial frame and subsequent retained updates. Zero-alpha pixels pass input
through; `AlphaHitTest::PassThrough` makes the entire surface ignore pointer input.

`WindowLayer::Desktop { follow_show_desktop: true }` selects non-activating,
taskbar-free desktop placement. The backend tracks desktop foreground transitions
and re-establishes its position after `TaskbarCreated`, emitting
`NativeWindowEvent::DesktopRecreated`. Application-requested hiding remains in
effect during later desktop transitions. The window keeps independent ownership
instead of becoming an Explorer child or owned window, so its application state
does not depend on the lifetime of an Explorer HWND.

Desktop windows must be undecorated and cannot be always-on-top. Normal application
windows retain their existing activation and stacking behavior.

## Overlay windows

`WindowLayer::Overlay { activate_on_click }` selects a topmost, undecorated
window intended for desktop pets, HUDs and other desktop overlays. Overlay
windows require `desktop-overlay` and can combine with
`WindowSurface::PerPixelAlpha`. `activate_on_click=false` keeps the overlay
non-activating; `true` allows a click to activate it for keyboard interaction.

Use `WindowSpec::position` / `NativeWindowBuilder::position` for the initial
logical placement and `AppCx::move_window_to` for runtime movement. Positions use
the virtual-desktop top-left as the origin with `y` growing downward; native
hosts convert them with the target monitor's scale.

`NativeWindowEvent::MetricsChanged(WindowMetrics)` reports the current outer
bounds, monitor work area and DPI after moves, resizes and scale changes. The
application remains authoritative for where the overlay should go.

Platform behavior:

- **Win32:** layered windows use `WS_EX_LAYERED`, topmost/tool-window styles and
  `WS_EX_NOACTIVATE` when `activate_on_click=false`. `AlphaHitTest::TransparentPixels`
  relies on the layered-window alpha hit test; `PassThrough` adds
  `WS_EX_TRANSPARENT`. Target proof is pending external validation.
- **AppKit:** borderless floating windows use a clear background and
  `drawRect:` skips the opaque page fill. `PassThrough` sets
  `ignoresMouseEvents`; `TransparentPixels` renders an offscreen alpha mask from
  the same draw plan, overrides `hitTest:` and toggles `ignoresMouseEvents` from
  a global mouse monitor. Target proof is pending external validation.
- **Linux Direct (X11/XWayland):** overlay windows are presented through an ARGB
  depth-32 buffer, keep the frame topmost, and use an XShape input region derived
  from the same alpha mask. Overlay windows select the X11 backend; on a Wayland
  session this means XWayland.
- **Native Wayland (`linux-wayland-layer`):** overlay-only applications create
  `zwlr_layer_surface_v1` surfaces with ARGB8888 `wl_shm` buffers and
  `wl_surface.set_input_region`. GNOME/KDE sessions without the protocol fall
  back to XWayland. IME, menus, dialogs and accessibility are outside this
  overlay-only backend in its first revision.
- **GTK4 / `linux-direct-lite`:** overlays are unsupported in this revision.

Application code must not branch on the display server. Unsupported overlay
requests are rejected by the selected backend or degraded through
`WindowSpec::resolve_for`; they are never silently presented as a different
window model.

## Native menus

`UpdateContext::shell_menu` queues a `ShellMenuSpec` and delivers a typed
`ShellMenuOutcome`. The native menu includes Shell verbs and application
`MenuSpec` entries. Application selections return `Command` values; product
operations remain in the application's update function.

Native adapter modules can borrow an external HMENU through `NativeMenu`, then
call `MenuSpec::merge_into`. The insertion guard provides the native-ID map and
removes only its own rows when dropped. Existing commands and external additions
are preserved. `NativeMenuHook` forwards menu initialization, measurement, drawing
and character messages for the lifetime of a tracking session.

Menu and drag effects execute outside retained View and route locks. Window
destruction terminates outcome delivery through the existing effect lifetime.

## File transfers

`on_drop_files` receives `FilesDropped` messages inside the node's clipped bounds.
It accepts copy operations by default. `on_drop_files_with` declares additional
permitted operations; keyboard modifiers and the source's allowed effects
determine the candidate operation.

Move and link targets must complete their operation synchronously and call
`UpdateContext::accept_drop` with the negotiated effect before returning from the
drop update. `reject_drop` declines the operation. Asynchronous processing can use
the non-destructive copy contract. The framework transports data and reports the
negotiated result; it does not copy, move or delete application files.

`UpdateContext::start_drag(DragPayload::ShellItems(items), effects, respond)` starts
the native drag loop and reports cancellation or the accepted operation. Start it
from an appropriate pointer gesture while the source button remains pressed.
Shell drag images use the system helper when available.

`NativeClipboardService` reads and writes `ClipboardData::Files` with
`clipboard-files`. File clipboard paths must be absolute filesystem paths.
Canonical disk and UNC paths use equivalent Shell parsing names; device paths
and verbatim names that would change meaning are rejected.
Incoming file lists use CF_HDROP; non-UTF-8 paths produce an explicit clipboard
error instead of a lossy pathname. Drag messages retain native `PathBuf` values.
Text and image clipboard requests keep their existing path. The Shell services
initialize and release an STA OLE apartment on their calling thread.

## Appearance and rendering

`PlatformPalette::current(mode)` returns the same resolved color roles used by the
active Win32 renderer. It honors system high contrast and avoids duplicating
application palettes. `NativeWindowEvent::ThemeChanged` supplies updated snapshots.
Native edit controls, trees, lists, menus and asynchronously created Explorer views
receive the owning window's appearance.

The optional Direct2D renderer consumes the existing draw protocol, including
clipping, images, rounded geometry and arcs. Text and semantic icons retain the
shared native shaping and fallback paths. `WindowRenderer::Direct2dHardware` is
an explicit application choice; `Direct2dSoftware` avoids selecting a hardware
render target. Device/DC resources belong to the window and are released at
teardown.

File-management icons are available as `Back`, `Forward`, `Up`, `Sort`, `View`,
`Rename`, `Share`, `Maximize`, `Restore`, `Preview`, `Properties`, `Star` and
`StarFilled`. `Close` remains the existing close icon. Each icon has native symbol
mappings, a licensed SVG fallback and a normalized vector fallback.

## Global shortcuts

`global_hotkey(HotkeySpec)` registers a system-wide shortcut for the window's
lifetime. Duplicate or unavailable combinations return an error with the native
failure code. Destruction unregisters owned bindings.

Use `stateful_with_commands` to map `Command` values to application messages when
the shortcut must remain effective while `release_view_when_hidden` is active.
That command handler belongs to application state independently of the retained
View. `NativeWindowEvent::GlobalHotkey` is also available as a View notification.

## Example and verification

```powershell
cargo run --example native_integration --no-default-features --features windows-shell,window-chrome,label,dark-mode,hotkey -- <folder> --dark
```

The example embeds an edit control, a namespace tree and an Explorer view. Its
Ctrl+Alt+Shift+F9 shortcut hides and restores the window through typed commands.

The interactive target test requires an absolute `ZSUI_INTEGRATION_PROOF_DIR`:

```powershell
cargo test --features full --lib native_windows_integration_target_proof -- --ignored --nocapture
```

It records real window captures, Unicode input, native focus traversal, global
hotkey delivery, OLE file drop, menu-message forwarding and a visible desktop body
pixel after a paired Win+D operation. File clipboard round trips run in a separate
process and noninteractive window station. Recovery testing delivers
`TaskbarCreated` to the target window; it does not restart Explorer. The report
uses `zsui.windows-integration-proof/v1` and retains its capture mechanism and
target architecture.

The overlay example is platform-neutral:

```powershell
cargo run --example desktop_pet --features "window,canvas,desktop-overlay"
```

Its X11 smoke writes a PNG plus `native_overlay` evidence for depth, alpha
coverage, XShape input rectangles, position and work area:

```powershell
cargo run --example desktop_pet --features "window,canvas,desktop-overlay,native-smoke" -- --smoke target/desktop-pet-proof
```

On Linux, add `linux-wayland-layer` to exercise the overlay-only layer-shell
backend on a compositor exposing `zwlr_layer_shell_v1`; otherwise the example
uses X11/XWayland. Win32 and AppKit overlay target interaction proof remains
external validation work in this revision.

Related native contracts: [DWM custom frames](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe),
[layered-window presentation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow),
[Shell data transfer](https://learn.microsoft.com/en-us/windows/win32/shell/dragdrop),
and [native menu tracking](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-trackpopupmenu).

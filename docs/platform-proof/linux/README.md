# Linux platform proof

Backend: `linux-direct`

The Ubuntu CI job launches a real X11 window under Xvfb, presents the Cairo
software surface with the shared ZSUI HarfRust/Swash text engine
(`linux-direct-rust-text`), and emits PNG plus structured proof JSON. The
Pango/Cairo text path remains available as the compatibility fallback when the
Rust-text feature is not enabled. The separate real Weston Wayland, AT-SPI and
menu proof is also operational. Final completion still requires the full
artifact set and reviewed regression baselines defined by the parent manifest.

Reviewed documentation evidence:

- `gallery-inputs-light.png` and `.json`: the shared Gallery scene captured from
  the final X11 Softbuffer surface with Cairo drawing and the shared ZSUI text
  engine.
- `notepad-interaction.png` and `.json`: the shared Notepad interaction captured
  from the same default Linux profile.
- `notepad-interaction-lite.png` and `.json`: the unchanged Notepad source using
  the same text engine over tiny-skia and Softbuffer.

The checked-in images above were captured before the `linux-direct-rust-text`
profile switch; the next `native-proof` run refreshes their text rendering. The
optional `bundled-ui-font` feature is not part of the default Ubuntu-native
proof and must be identified as bundled-font evidence when used.

The workflow also launches a real Weston Wayland compositor with `DISPLAY`
unset, verifies the display handle, exercises an external AT-SPI client and
probes the native menu surface. Those per-run artifacts remain in CI rather than
being duplicated in this small documentation set.

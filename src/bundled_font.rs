//! Optional deterministic UI family shared by the Linux Rust-text presenters.
//!
//! The face is embedded only when `bundled-ui-font` is enabled. CJK, emoji and
//! complex scripts continue to resolve through host fallback.

pub(crate) const FAMILY: &str = "Inter";
pub(crate) const DATA: &[u8] = include_bytes!("../assets/fonts/InterVariable.ttf");

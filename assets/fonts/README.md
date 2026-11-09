# Bundled UI font

`InterVariable.ttf` is the Inter variable UI font.

- Source: https://github.com/google/fonts/tree/main/ofl/inter
- Upstream: https://github.com/rsms/inter
- License: SIL Open Font License 1.1, see `Inter-OFL.txt`
- SHA-256: `29160a80ff49ddcab2c97711247e08b1fab27a484a329ce8b813d820dc559031`

The font is embedded only when the optional `bundled-ui-font` Cargo feature is
enabled. It gives the shared ZSUI text engine a deterministic Latin/Greek/
Cyrillic UI family; CJK, emoji and other scripts continue to resolve through
the host system's font fallback. The feature does not replace the default
host-system font contract.

# LightDriver logo concepts

The latest revision is `lightdriver-logo-arches-metro.png`: three symmetrical concentric blue, coral-red and green arches with a common baseline, in the requested flat suede/Metro direction. Both horizontal and vertical stems were removed at Moosa's request. The background is transparent.

The original `lightdriver-logo-arches.png` remains for comparison. The approved final image is now embedded in both native app headers and used for window/taskbar icons. The Windows executable resource embeds lightdriver.ico for Explorer. A shared Rust branding adapter caches decoded pixels and the image handle; the PNG stays proportionate and transparent.

Created/edited with the built-in image generation tool, without CLI fallback.

## Final edit prompt

Use case: precise-object-edit / logo-brand.
Edit target: supplied symmetrical three-arch LightDriver logo.
USER CORRECTION: REMOVE BOTH THE HORIZONTAL AND VERTICAL WHITE STEMS COMPLETELY. Keep ONLY the three colored arches, absolutely nothing else. This is an object-removal edit, no white material should survive anywhere.
Reconstruct each colored arch uninterrupted where the vertical stem crossed it. Finish all six arch endpoints as neat flat horizontal cuts at exactly the same common baseline. The result is three concentric, uniformly thick, evenly spaced semicircular arches, precisely symmetrical about the vertical center. Preserve the blue outer, coral-red middle, and green inner colors and restrained flat 2D Metro/suede matte style.
Everywhere outside the three colored arches, including the entire center opening, gaps between arches, and below their common baseline, is fully transparent. Remove every white line, white speck, residue, fringe or patch. No cross, no axis, no stems, no underline, no pedestal. Clean vector-like antialiasing, no glow, no fabric texture, no shading, no 3D, no text, no border. Center the remaining arches with generous transparent padding.



## Native icon packaging

Run cargo run -p lightdriver-branding --example package_icon when the approved source asset changes. This Rust tool resizes and letterboxes the existing mark into a transparent 256 × 256 PNG-backed ICO; it preserves the design. Windows builds embed that icon using [winresource](https://docs.rs/winresource/0.1.31/winresource/). The Windows SDK resource compiler is a build dependency; no external asset files or image decoder applications are needed at runtime.

The logo is cached once per UI process. GPU rendering remains the default.

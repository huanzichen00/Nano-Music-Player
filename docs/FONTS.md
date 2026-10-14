# Font selection

Settings → Display → Font offers Classic, Maple Mono and JetBrains Mono.
A selection applies immediately to the interface, music metadata and lyrics, and persists in
`settings.json`. Classic keeps Helvetica Neue; systems without that family use
Slint's system fallback. On macOS the application defaults to Skia for text rendering. Native
comparisons at the installed window scale reproduced broken small-text strokes
with the OpenGL FemtoVG renderer for Helvetica Neue, Helvetica and Arial; the
same widget fixture looked clean with Skia, without changing the font family.
The diagnostic fixture is `examples/font_render_audit.rs`; set
`SLINT_BACKEND=winit-femtovg` to compare the previous renderer. Explicit backend
overrides remain supported. Other platforms retain their existing renderer.
Old settings default to Classic.
Font changes leave the stored metadata, lyric content and timestamps untouched.

The picker previews each family with Latin letters (including Ss), digits,
Japanese and Chinese. Maple Mono and JetBrains Mono are bundled as unmodified
regular, semibold and bold TTFs; users do not need to install them. System fonts
provide missing CJK glyphs. Lyrics retain the existing 13-unit size and active
weight; romaji retains its 10-unit size. Each line uses consistent typography,
without character-specific scaling. Native macOS menus retain the system menu font.

The app uses Slint's [custom-font imports and window font defaults](https://docs.slint.dev/latest/docs/slint/guide/development/fonts/).
The bundled sources are [Maple Mono v7.9](https://github.com/subframe7536/maple-font/releases/tag/v7.9)
and [JetBrains Mono v2.304](https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304).
Their complete SIL Open Font License notices are retained beside the assets in
`ui/fonts/maple-mono/LICENSE.txt` and `ui/fonts/jetbrains-mono/LICENSE.txt`.

Verification includes old-config migration, all-choice round trips, returning to
the selected Settings row, controller restart, unchanged music text, and rendered
glyph differences. The visual fixtures show Latin, Japanese and Chinese lyrics;
native Windows/Linux font fallback is not verified on this macOS host.

The observed native artifacts resemble Slint’s documented [half-pixel glyph
texture sampling issue](https://github.com/slint-ui/slint/issues/6298). This is
a diagnosis from the side-by-side native fixtures, not a claim that its earlier
upstream fix covers this particular machine and Slint 1.18.1 build.

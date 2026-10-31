# Slint iOS background rendering backport

These are the unmodified published `i-slint-backend-winit` and
`i-slint-renderer-skia` **1.18.1** crates, with the 45-line iOS fix from:

- Issue: https://github.com/slint-ui/slint/issues/14086
- Draft upstream PR: https://github.com/slint-ui/slint/pull/14094
- Commit: `383c344c19a1934e266b99d1df860c3ed85fac93` (2026-10-15)

The phone can keep running for audio in the background. UIKit rejects Metal
commands during that period, but Skia can retain atlas entries for uploads that
never ran. The backport blocks drawing at the backend's `draw()` entry point
(including the display-link path), requests the skipped frame on return, and
frees GPU caches before the next foreground render. It preserves audio playback,
media controls, the pinned Slint version and the GPU renderer.

The two window-adapter hunks were adapted to the 1.18.1 layout (macOS-only draw
and boolean occlusion payload). The remaining upstream hunks apply unchanged.
`slint-ios-background.patch` records the exact backport against published 1.18.1.
Both crates retain their upstream copyright headers and complete LICENSES.

Remove these path patches after adopting a released Slint version containing
this upstream fix. Compilation and portable UI tests do not prove physical
phone lock/unlock behavior; that scenario still requires a device run.

## Windows frameless menus

`i-slint-backend-winit/winitwindowadapter.rs` also disables the native Windows
menubar for this application. Winit 0.30.13 adds the attached `HMENU` height in
`WindowFlags::adjust_rect`, while its undecorated `WM_NCCALCSIZE` handler treats
the full outer rectangle as client area. Each requested client height can thus
return larger by one menu height, which the player's aspect correction converts
into another zoom and resize request.

Windows uses Slint's in-window menubar in both Device and Screen modes, including
when decorations change at runtime. Context menus retain their native support;
macOS retains its system menubar. `tests/windows_window_resize.rs` exercises the
real Windows backend at several scale factors in the release workflow, covering
idle startup, mode changes and external resize requests.

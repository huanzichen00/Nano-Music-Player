#[cfg(any(target_os = "ios", feature = "mobile"))]
pub mod ios;
#[cfg(all(target_os = "macos", not(feature = "mobile")))]
pub(crate) mod macos_hotkey;
pub mod macos_media;

/// Keep both frontends on the same native renderer and honor diagnostic overrides.
pub fn select_ui_backend() -> Result<(), slint::PlatformError> {
    // The OpenGL glyph atlas tears text at fractional macOS window scales.
    // Slint's iOS backend likewise uses Winit + Skia.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    if std::env::var_os("SLINT_BACKEND").is_none() {
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("skia".into())
            .select()?;
    }
    Ok(())
}

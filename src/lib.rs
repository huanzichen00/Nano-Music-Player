pub mod ai;
#[cfg(not(any(target_os = "ios", feature = "mobile")))]
pub mod app;
pub mod audio;
pub mod genius;
#[cfg(not(any(target_os = "ios", feature = "mobile")))]
pub mod input;
pub mod library;
#[cfg(any(target_os = "ios", feature = "mobile"))]
pub mod mobile;
pub mod platform;
#[cfg(not(any(target_os = "ios", feature = "mobile")))]
pub mod podcast;
pub mod shared;
pub mod translation;
#[cfg(not(any(target_os = "ios", feature = "mobile")))]
pub mod video;

pub mod ui {
    slint::include_modules!();
}

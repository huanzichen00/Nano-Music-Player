//! UI-independent request, cache and persistence owners used by both frontends.
// These services accept domain data and return completions; UI models, navigation
// and playback coordination remain with the desktop and mobile controllers.
#[cfg_attr(any(target_os = "ios", feature = "mobile"), allow(dead_code))]
pub(crate) mod genius;
#[cfg_attr(any(target_os = "ios", feature = "mobile"), allow(dead_code))]
pub(crate) mod lyrics;
#[cfg_attr(any(target_os = "ios", feature = "mobile"), allow(dead_code))]
pub(crate) mod playlists;
#[cfg_attr(any(target_os = "ios", feature = "mobile"), allow(dead_code))]
pub(crate) mod statistics;

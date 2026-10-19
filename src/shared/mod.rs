//! Platform-independent application contracts, shared by desktop and mobile.
// Keep the original source locations so their catalogs and resource audits
// remain unchanged, while both frontends use exactly the same implementations.
#[path = "../app/action.rs"]
pub mod action;
pub(crate) mod artwork;
#[path = "../app/config.rs"]
pub mod config;
pub(crate) mod diagnostics;
#[path = "../app/i18n.rs"]
pub mod i18n;
#[path = "../app/navigation.rs"]
pub mod navigation;
pub mod settings;
#[path = "../app/typography.rs"]
pub mod typography;

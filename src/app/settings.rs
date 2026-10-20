//! Desktop adapter for the shared settings catalog and value presentation.

use super::navigation::{Screen, SettingsSection};
use super::state::{Row, State, Target};
use crate::shared::settings::{self, Target as SharedTarget, Values};

impl State {
    pub(crate) fn is_settings_list(&self) -> bool {
        settings::is_settings_list(&self.navigation.current().screen)
    }

    pub(super) fn settings_rows(&self) -> Option<Vec<Row>> {
        let screen = &self.navigation.current().screen;
        let mut rows = settings::rows(
            screen,
            self.current
                .and_then(|id| self.tracks.get(id))
                .map(|track| track.title.as_str()),
        )?;
        if *screen == Screen::Settings {
            rows.push(settings::Row {
                title: "Translate Current Song".into(),
                subtitle: "Generate or remove its translation".into(),
                target: SharedTarget::Screen(Screen::SettingsSection(SettingsSection::Translation)),
            });
            rows.push(settings::Row {
                title: "Refresh Library".into(),
                subtitle: String::new(),
                target: SharedTarget::Action(super::action::Action::RefreshLibrary),
            });
        }
        Some(
            rows.into_iter()
                .map(|row| Row {
                    title: row.title,
                    subtitle: row.subtitle,
                    artwork_id: None,
                    target: row.target.into(),
                })
                .collect(),
        )
    }

    pub(crate) fn setting_toggle(&self, target: &Target) -> Option<bool> {
        Values::from(self).toggle(&shared_target(target)?)
    }

    pub(crate) fn setting_selected(&self, target: &Target) -> bool {
        shared_target(target).is_some_and(|target| Values::from(self).selected(&target))
    }

    pub(crate) fn setting_value(&self, target: &Target) -> String {
        shared_target(target)
            .map(|target| Values::from(self).value(&target))
            .unwrap_or_default()
    }
}

impl From<&State> for Values {
    fn from(state: &State) -> Self {
        Self {
            language: state.language,
            font: state.font,
            smooth_slides: state.smooth_slides,
            always_on_top: state.always_on_top,
            gapless_playback: state.gapless_playback,
            replay_gain_prevent_clipping: state.replay_gain_prevent_clipping,
            romaji_visible: state.romaji_visible,
            translation_visible: state.translation_visible,
            lyrics_visible: state.lyrics_visible,
            device_mode: state.device_mode,
            zoom: state.zoom,
            replay_gain_mode: state.replay_gain_mode,
            replay_gain_preamp_db: state.replay_gain_preamp_db,
            translation_target: state.translation_target,
        }
    }
}

impl From<SharedTarget> for Target {
    fn from(target: SharedTarget) -> Self {
        match target {
            SharedTarget::Screen(value) => Self::Screen(value),
            SharedTarget::Action(value) => Self::Action(value),
            SharedTarget::Language(value) => Self::Language(value),
            SharedTarget::Font(value) => Self::Font(value),
            SharedTarget::DisplayMode(value) => Self::DisplayMode(value),
            SharedTarget::Zoom(value) => Self::Zoom(value),
            SharedTarget::ReplayGain(value) => Self::ReplayGain(value),
            SharedTarget::ReplayGainPreamp(value) => Self::ReplayGainPreamp(value),
            SharedTarget::Translation(value) => Self::Translation(value),
        }
    }
}

fn shared_target(target: &Target) -> Option<SharedTarget> {
    Some(match target {
        Target::Screen(value) => SharedTarget::Screen(value.clone()),
        Target::Action(value) => SharedTarget::Action(*value),
        Target::Language(value) => SharedTarget::Language(*value),
        Target::Font(value) => SharedTarget::Font(*value),
        Target::DisplayMode(value) => SharedTarget::DisplayMode(*value),
        Target::Zoom(value) => SharedTarget::Zoom(*value),
        Target::ReplayGain(value) => SharedTarget::ReplayGain(*value),
        Target::ReplayGainPreamp(value) => SharedTarget::ReplayGainPreamp(*value),
        Target::Translation(value) => SharedTarget::Translation(*value),
        _ => return None,
    })
}

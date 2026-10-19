//! Settings navigation and presentation. Persistent preferences stay separate
//! from commands that act on the currently playing song.
use super::{
    action::Action,
    i18n::Language,
    navigation::{Screen, SettingsSection as Section},
    state::{Row, State, Target},
    typography::Font,
};
use crate::{
    audio::gain::GainMode,
    translation::{Command, TargetLanguage},
};

fn row(title: &str, subtitle: &str, target: Target) -> Row {
    Row {
        title: title.into(),
        subtitle: subtitle.into(),
        artwork_id: None,
        target,
    }
}
fn page(title: &str, subtitle: &str, section: Section) -> Row {
    row(
        title,
        subtitle,
        Target::Screen(Screen::SettingsSection(section)),
    )
}

impl State {
    pub(crate) fn is_settings_list(&self) -> bool {
        matches!(
            self.navigation.current().screen,
            Screen::Settings | Screen::SettingsSection(_) | Screen::Languages | Screen::Fonts
        )
    }

    pub(super) fn settings_rows(&self) -> Option<Vec<Row>> {
        let action = |title, description, action| row(title, description, Target::Action(action));
        Some(match self.navigation.current().screen {
            Screen::Settings => vec![
                page(
                    "Appearance",
                    "Window style, size and font",
                    Section::Display,
                ),
                page(
                    "Playback",
                    "Track transitions and volume",
                    Section::Playback,
                ),
                page(
                    "Lyrics",
                    "Display, translation and song lyrics",
                    Section::Lyrics,
                ),
                row(
                    "Language",
                    "Choose the app language",
                    Target::Screen(Screen::Languages),
                ),
                row(
                    "Keyboard Shortcuts",
                    "Change keys for any action",
                    Target::Screen(Screen::Shortcuts),
                ),
                action(
                    "AI Services",
                    "DeepSeek key for lyrics and Genius",
                    Action::ConfigureDeepSeek,
                ),
                page(
                    "Translate Current Song",
                    "Generate or remove its translation",
                    Section::Translation,
                ),
            ],
            Screen::SettingsSection(Section::Display) => vec![
                page(
                    "Window Style",
                    "Show the iPod body or screen only",
                    Section::DisplayMode,
                ),
                page(
                    "Window Size",
                    "Choose how large the player appears",
                    Section::WindowSize,
                ),
                row(
                    "Font",
                    "Text style throughout the app",
                    Target::Screen(Screen::Fonts),
                ),
                action(
                    "Page Animation",
                    "Slide between pages when navigating",
                    Action::ToggleSmoothSlides,
                ),
                action(
                    "Keep on Top",
                    "Keep the player above other windows",
                    Action::ToggleAlwaysOnTop,
                ),
            ],
            Screen::SettingsSection(Section::DisplayMode) => vec![
                row(
                    "iPod Body",
                    "Screen with bezel and Home button",
                    Target::DisplayMode(true),
                ),
                row(
                    "Screen Only",
                    "A plain window without the iPod body",
                    Target::DisplayMode(false),
                ),
            ],
            Screen::SettingsSection(Section::WindowSize) => {
                [0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0]
                    .into_iter()
                    .map(|zoom| {
                        row(
                            &format!("{:.0}%", zoom * 100.0),
                            if zoom == 1.0 {
                                "Original size"
                            } else {
                                "Scale the whole player"
                            },
                            Target::Zoom(zoom),
                        )
                    })
                    .collect()
            }
            Screen::SettingsSection(Section::Playback) => vec![
                action(
                    "Gapless Playback",
                    "Play consecutive tracks without pauses",
                    Action::ToggleGaplessPlayback,
                ),
                page(
                    "Volume Leveling",
                    "Balance loudness using ReplayGain tags",
                    Section::ReplayGain,
                ),
                page(
                    "Volume Boost",
                    "Adjust ReplayGain level before playback",
                    Section::ReplayGainPreamp,
                ),
                action(
                    "Prevent Distortion",
                    "Limit peaks when volume leveling is on",
                    Action::ToggleReplayGainClipping,
                ),
            ],
            Screen::SettingsSection(Section::ReplayGain) => vec![
                row(
                    "Off",
                    "Keep the original loudness",
                    Target::ReplayGain(GainMode::Off),
                ),
                row(
                    "By Song",
                    "Match loudness across individual songs",
                    Target::ReplayGain(GainMode::Track),
                ),
                row(
                    "By Album",
                    "Preserve volume differences in an album",
                    Target::ReplayGain(GainMode::Album),
                ),
            ],
            Screen::SettingsSection(Section::ReplayGainPreamp) => (-4..=4)
                .map(|step| {
                    let db = step as f32 * 3.0;
                    row(
                        &format!("{db:+.0} dB"),
                        if step == 0 {
                            "No extra boost"
                        } else {
                            "Applied to audio with ReplayGain tags"
                        },
                        Target::ReplayGainPreamp(db),
                    )
                })
                .collect(),
            Screen::SettingsSection(Section::Lyrics) => vec![
                action(
                    "Show Translation",
                    "Display a saved translation below lyrics",
                    Action::ToggleTranslation,
                ),
                page(
                    "Translate To",
                    "Language used for new translations",
                    Section::TranslationLanguage,
                ),
                action(
                    "Japanese Readings",
                    "Show romaji when translation is hidden",
                    Action::ToggleRomaji,
                ),
                page(
                    "Translate Current Song",
                    "Generate or remove its translation",
                    Section::Translation,
                ),
                row(
                    "Find / Edit Lyrics",
                    "Choose any song from your library",
                    Target::Screen(Screen::LyricSongs),
                ),
            ],
            Screen::SettingsSection(Section::Translation) => vec![
                row(
                    "Translate Lyrics",
                    &self
                        .current
                        .and_then(|id| self.tracks.get(id))
                        .map(|track| track.title.clone())
                        .unwrap_or_else(|| "Play a song to translate its lyrics".into()),
                    Target::Translation(Command::Translate),
                ),
                row(
                    "Regenerate Translation",
                    "Replace the current song's translation",
                    Target::Translation(Command::Regenerate),
                ),
                row(
                    "Cancel Translation",
                    "Stop the pending lyric request",
                    Target::Translation(Command::Cancel),
                ),
                row(
                    "Remove Translation",
                    "Delete this song's saved translation",
                    Target::Translation(Command::Remove),
                ),
            ],
            Screen::SettingsSection(Section::TranslationLanguage) => TargetLanguage::ALL
                .into_iter()
                .map(|language| {
                    row(
                        language.label(),
                        "",
                        Target::Translation(Command::SetTarget(language)),
                    )
                })
                .collect(),
            Screen::Languages => Language::ALL
                .into_iter()
                .map(|language| row(language.label(), "", Target::Language(language)))
                .collect(),
            Screen::Fonts => Font::ALL
                .into_iter()
                .map(|font| {
                    row(
                        font.label(),
                        "Aa Bb Cc Ss 0123 · 日本語 中文",
                        Target::Font(font),
                    )
                })
                .collect(),
            _ => return None,
        })
    }

    pub(crate) fn setting_toggle(&self, target: &Target) -> Option<bool> {
        Some(match target {
            Target::Action(Action::ToggleSmoothSlides) => self.smooth_slides,
            Target::Action(Action::ToggleAlwaysOnTop) => self.always_on_top,
            Target::Action(Action::ToggleGaplessPlayback) => self.gapless_playback,
            Target::Action(Action::ToggleReplayGainClipping) => self.replay_gain_prevent_clipping,
            Target::Action(Action::ToggleRomaji) => self.romaji_visible,
            Target::Action(Action::ToggleTranslation) => self.translation_visible,
            _ => return None,
        })
    }

    pub(crate) fn setting_selected(&self, target: &Target) -> bool {
        match target {
            Target::Language(value) => self.language == *value,
            Target::Font(value) => self.font == *value,
            Target::DisplayMode(value) => self.device_mode == *value,
            Target::Zoom(value) => (self.zoom - value).abs() < 0.005,
            Target::ReplayGain(value) => self.replay_gain_mode == *value,
            Target::ReplayGainPreamp(value) => (self.replay_gain_preamp_db - value).abs() < 0.005,
            Target::Translation(Command::SetTarget(value)) => self.translation_target == *value,
            _ => false,
        }
    }

    pub(crate) fn setting_value(&self, target: &Target) -> String {
        match target {
            Target::Screen(Screen::Languages) => self.language.label().into(),
            Target::Screen(Screen::Fonts) => self.font.label().into(),
            Target::Screen(Screen::SettingsSection(section)) => match section {
                Section::DisplayMode => if self.device_mode {
                    "iPod Body"
                } else {
                    "Screen Only"
                }
                .into(),
                Section::WindowSize => format!("{:.0}%", self.zoom * 100.0),
                Section::ReplayGain => match self.replay_gain_mode {
                    GainMode::Off => "Off",
                    GainMode::Track => "By Song",
                    GainMode::Album => "By Album",
                }
                .into(),
                Section::ReplayGainPreamp => format!("{:+.0} dB", self.replay_gain_preamp_db),
                Section::TranslationLanguage => self.translation_target.label().into(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }
}

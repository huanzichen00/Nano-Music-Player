//! Settings menu data and presentation shared by desktop and mobile.
//! Platform adapters execute targets and decide which options are available.

use super::{
    action::Action,
    config::Settings,
    i18n::Language,
    navigation::{Screen, SettingsSection as Section},
    typography::Font,
};
use crate::{
    audio::gain::GainMode,
    translation::{Command, TargetLanguage},
};

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Screen(Screen),
    Action(Action),
    Language(Language),
    Font(Font),
    DisplayMode(bool),
    Zoom(f32),
    ReplayGain(GainMode),
    ReplayGainPreamp(f32),
    Translation(Command),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub title: String,
    pub subtitle: String,
    pub target: Target,
}

/// A lightweight snapshot independent from either frontend's controller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Values {
    pub language: Language,
    pub font: Font,
    pub smooth_slides: bool,
    pub always_on_top: bool,
    pub gapless_playback: bool,
    pub replay_gain_prevent_clipping: bool,
    pub romaji_visible: bool,
    pub translation_visible: bool,
    pub lyrics_visible: bool,
    pub device_mode: bool,
    pub zoom: f32,
    pub replay_gain_mode: GainMode,
    pub replay_gain_preamp_db: f32,
    pub translation_target: TargetLanguage,
}

impl From<&Settings> for Values {
    fn from(settings: &Settings) -> Self {
        Self {
            language: settings.language,
            font: settings.font,
            smooth_slides: settings.smooth_slides,
            always_on_top: settings.always_on_top,
            gapless_playback: settings.gapless_playback,
            replay_gain_prevent_clipping: settings.replay_gain_prevent_clipping,
            romaji_visible: settings.romaji_visible,
            translation_visible: settings.translation_visible,
            lyrics_visible: settings.lyrics_visible,
            device_mode: settings.device_mode,
            zoom: settings.zoom,
            replay_gain_mode: settings.replay_gain_mode,
            replay_gain_preamp_db: settings.replay_gain_preamp_db,
            translation_target: settings.translation_target,
        }
    }
}

pub fn is_settings_list(screen: &Screen) -> bool {
    matches!(
        screen,
        Screen::Settings | Screen::SettingsSection(_) | Screen::Languages | Screen::Fonts
    )
}

fn row(title: &str, subtitle: &str, target: Target) -> Row {
    Row {
        title: title.into(),
        subtitle: subtitle.into(),
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

pub fn rows(screen: &Screen, current_title: Option<&str>) -> Option<Vec<Row>> {
    let action = |title, description, action| row(title, description, Target::Action(action));
    Some(match screen {
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
        Screen::SettingsSection(Section::WindowSize) => [0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0]
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
            .collect(),
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
                current_title.unwrap_or("Play a song to translate its lyrics"),
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

impl Values {
    pub fn toggle(&self, target: &Target) -> Option<bool> {
        Some(match target {
            Target::Action(Action::ToggleSmoothSlides) => self.smooth_slides,
            Target::Action(Action::ToggleLyrics) => self.lyrics_visible,
            Target::Action(Action::ToggleAlwaysOnTop) => self.always_on_top,
            Target::Action(Action::ToggleGaplessPlayback) => self.gapless_playback,
            Target::Action(Action::ToggleReplayGainClipping) => self.replay_gain_prevent_clipping,
            Target::Action(Action::ToggleRomaji) => self.romaji_visible,
            Target::Action(Action::ToggleTranslation) => self.translation_visible,
            _ => return None,
        })
    }

    pub fn selected(&self, target: &Target) -> bool {
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

    pub fn value(&self, target: &Target) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_menu_and_current_value_follow_persisted_preferences() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let settings = Settings {
            language: Language::Japanese,
            font: Font::MapleMono,
            ..Settings::default()
        };
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path).unwrap();
        let values = Values::from(&loaded);
        let root = rows(&Screen::Settings, None).unwrap();
        assert!(
            root.iter()
                .any(|row| row.title == "Language"
                    && row.target == Target::Screen(Screen::Languages))
        );
        let languages = rows(&Screen::Languages, None).unwrap();
        let selected: Vec<_> = languages
            .iter()
            .filter(|row| values.selected(&row.target))
            .map(|row| row.title.as_str())
            .collect();
        assert_eq!(selected, ["日本語"]);
        assert!(languages.iter().any(|row| row.title == "English"));
        assert!(languages.iter().any(|row| row.title == "简体中文"));
        assert_eq!(values.value(&Target::Screen(Screen::Languages)), "日本語");
        assert_eq!(values.value(&Target::Screen(Screen::Fonts)), "Maple Mono");
    }

    #[test]
    fn older_settings_show_lyrics_by_default_and_new_choices_survive_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        std::fs::write(&path, r#"{"language":"simplified_chinese"}"#).unwrap();
        let mut settings = Settings::load(&path).unwrap();
        assert!(settings.lyrics_visible);
        assert_eq!(settings.language, Language::SimplifiedChinese);
        settings.lyrics_visible = false;
        settings.save(&path).unwrap();
        let restored = Settings::load(&path).unwrap();
        assert!(!restored.lyrics_visible);
        assert_eq!(
            Values::from(&restored).toggle(&Target::Action(Action::ToggleLyrics)),
            Some(false)
        );
        // Desktop lyrics menus keep their existing controls; mobile may add a
        // visibility row without changing the common catalog.
        assert!(
            rows(&Screen::SettingsSection(Section::Lyrics), None)
                .unwrap()
                .iter()
                .all(|row| row.target != Target::Action(Action::ToggleLyrics))
        );
    }

    #[test]
    fn shared_values_render_toggles_radio_choices_and_summary_values() {
        let settings = Settings {
            smooth_slides: false,
            device_mode: false,
            zoom: 1.5,
            gapless_playback: false,
            replay_gain_mode: GainMode::Album,
            replay_gain_preamp_db: -3.0,
            translation_target: TargetLanguage::SimplifiedChinese,
            ..Settings::default()
        };
        let values = Values::from(&settings);
        assert_eq!(
            values.toggle(&Target::Action(Action::ToggleSmoothSlides)),
            Some(false)
        );
        assert_eq!(
            values.toggle(&Target::Action(Action::ToggleGaplessPlayback)),
            Some(false)
        );
        assert_eq!(values.toggle(&Target::Action(Action::TogglePlay)), None);
        for (section, expected) in [
            (Section::DisplayMode, "Screen Only"),
            (Section::WindowSize, "150%"),
            (Section::ReplayGain, "By Album"),
            (Section::ReplayGainPreamp, "-3 dB"),
            (Section::TranslationLanguage, "简体中文"),
        ] {
            assert_eq!(
                values.value(&Target::Screen(Screen::SettingsSection(section))),
                expected
            );
            let selected = rows(&Screen::SettingsSection(section), None)
                .unwrap()
                .into_iter()
                .filter(|row| values.selected(&row.target))
                .count();
            assert_eq!(selected, 1, "each choice page has one current selection");
        }
        assert!(!values.selected(&Target::Zoom(1.25)));
        assert!(!values.selected(&Target::ReplayGain(GainMode::Track)));
    }

    #[test]
    fn translation_menu_uses_the_current_song_without_controller_dependencies() {
        let screen = Screen::SettingsSection(Section::Translation);
        let idle = rows(&screen, None).unwrap();
        assert_eq!(idle[0].subtitle, "Play a song to translate its lyrics");
        let playing = rows(&screen, Some("夜に駆ける")).unwrap();
        assert_eq!(playing[0].subtitle, "夜に駆ける");
        assert_eq!(playing[0].target, Target::Translation(Command::Translate));
        assert!(
            playing
                .iter()
                .any(|row| row.target == Target::Translation(Command::Cancel))
        );
        assert!(rows(&Screen::Music, None).is_none());
        assert!(!is_settings_list(&Screen::NowPlaying));
    }
}

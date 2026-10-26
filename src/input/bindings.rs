use crate::app::action::Action;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Backspace,
    Space,
    Plus,
    Equals,
    Minus,
    Tab,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Function(u8),
    Character(char),
    Letter(char),
    Digit(char),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Modifiers {
    /// Slint normalizes this to Command on macOS and Control on Windows/Linux.
    pub primary: bool,
    /// Physical Control on macOS, the Windows/Super key on Windows/Linux.
    pub secondary: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub action: Action,
    pub key: Key,
    #[serde(default)]
    pub modifiers: Modifiers,
    /// Plus requires Shift on many layouts; match both forms without broadening other shortcuts.
    #[serde(default)]
    pub optional_shift: bool,
}

impl Binding {
    pub fn matches(&self, key: &Key, modifiers: Modifiers) -> bool {
        self.key == *key
            && self.modifiers.primary == modifiers.primary
            && self.modifiers.secondary == modifiers.secondary
            && self.modifiers.alt == modifiers.alt
            && (self.optional_shift || self.modifiers.shift == modifiers.shift)
    }

    pub fn label(&self, mac: bool) -> String {
        let mut parts = Vec::new();
        if self.modifiers.primary {
            parts.push(if mac { "⌘" } else { "Ctrl" }.to_owned());
        }
        if self.modifiers.secondary {
            parts.push(if mac { "Ctrl" } else { "Super" }.to_owned());
        }
        if self.modifiers.alt {
            parts.push(if mac { "Option" } else { "Alt" }.to_owned());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_owned());
        }
        parts.push(match self.key {
            Key::Up => "↑".to_owned(),
            Key::Down => "↓".to_owned(),
            Key::Left => "←".to_owned(),
            Key::Right => "→".to_owned(),
            Key::Enter => "Enter".to_owned(),
            Key::Escape => "Escape".to_owned(),
            Key::Backspace => "Backspace".to_owned(),
            Key::Space => "Space".to_owned(),
            Key::Plus => "+".to_owned(),
            Key::Equals => "=".to_owned(),
            Key::Minus => "−".to_owned(),
            Key::Tab => "Tab".into(),
            Key::Delete => "Delete".into(),
            Key::Home => "Home".into(),
            Key::End => "End".into(),
            Key::PageUp => "Page Up".into(),
            Key::PageDown => "Page Down".into(),
            Key::Function(n) => format!("F{n}"),
            Key::Character(c) => c.to_string(),
            Key::Letter(c) | Key::Digit(c) => c.to_uppercase().to_string(),
        });
        parts.join(" + ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keymap {
    pub version: u32,
    pub bindings: Vec<Binding>,
}

impl Default for Keymap {
    fn default() -> Self {
        use Action::{
            Activate, Back, CycleRepeat, DeviceMode, Home, ImportFolder, JumpToLetter, NextTrack,
            NowPlaying, PreviousTrack, ScreenMode, Search, SelectNext, SelectPrevious,
            ToggleLyrics, ToggleMute, TogglePlay, ToggleShuffle, VolumeDown, VolumeUp, ZoomIn,
            ZoomOut,
        };
        use Key::*;
        let plain = [
            (Up, SelectPrevious),
            (Down, SelectNext),
            (Letter('k'), SelectPrevious),
            (Letter('j'), SelectNext),
            (Enter, Activate),
            (Escape, Back),
            (Backspace, Back),
            (Letter('h'), Action::NavigateLeft),
            (Letter('l'), Action::NavigateRight),
            (Left, Action::Left),
            (Right, Action::Right),
            (Space, TogglePlay),
            (Letter('m'), ToggleMute),
            (Letter('s'), ToggleShuffle),
            (Letter('r'), CycleRepeat),
            (Letter('n'), NowPlaying),
            (Letter('y'), ToggleLyrics),
            (Letter('t'), Action::ToggleTranslation),
            (Letter('f'), JumpToLetter),
            (Letter('b'), Action::DeviceInformation),
            (Letter('d'), Action::TrackDetails),
        ];
        let primary = [
            (Right, NextTrack),
            (Left, PreviousTrack),
            (Up, VolumeUp),
            (Down, VolumeDown),
            (Letter('o'), ImportFolder),
            (Letter('f'), Search),
            (Digit('1'), DeviceMode),
            (Digit('2'), ScreenMode),
            (Plus, ZoomIn),
            (Equals, ZoomIn),
            (Minus, ZoomOut),
            (Character(','), Action::Settings),
        ];
        Self {
            version: 9,
            bindings: plain
                .into_iter()
                .map(|(key, action)| Binding {
                    action,
                    key,
                    modifiers: Modifiers::default(),
                    optional_shift: action == JumpToLetter,
                })
                .chain(primary.into_iter().map(|(key, action)| Binding {
                    optional_shift: matches!(key, Plus | Equals),
                    action,
                    key,
                    modifiers: Modifiers {
                        primary: true,
                        ..Modifiers::default()
                    },
                }))
                .chain(std::iter::once(Binding {
                    action: Home,
                    key: Letter('h'),
                    modifiers: Modifiers {
                        shift: true,
                        ..Modifiers::default()
                    },
                    optional_shift: false,
                }))
                .chain(std::iter::once(Binding {
                    action: Action::ShuffleAll,
                    key: Letter('s'),
                    modifiers: Modifiers {
                        shift: true,
                        ..Modifiers::default()
                    },
                    optional_shift: false,
                }))
                .chain(
                    [
                        (Letter('p'), Action::PracticeMode),
                        (Letter('m'), Action::PracticeAddMarker),
                        (Left, Action::PracticePreviousMarker),
                        (Right, Action::PracticeNextMarker),
                        (Letter('r'), Action::PracticeRestart),
                    ]
                    .into_iter()
                    .map(|(key, action)| Binding {
                        action,
                        key,
                        modifiers: if action == Action::PracticeMode {
                            Modifiers::default()
                        } else {
                            Modifiers {
                                primary: true,
                                alt: true,
                                ..Modifiers::default()
                            }
                        },
                        optional_shift: false,
                    }),
                )
                .chain(std::iter::once(Binding {
                    action: Action::PresentationMode,
                    key: Letter('q'),
                    modifiers: Modifiers::default(),
                    optional_shift: false,
                }))
                .collect(),
        }
    }
}

impl Keymap {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.version, 1..=9) {
            return Err(format!(
                "Unsupported shortcut file version: {}",
                self.version
            ));
        }
        for (i, binding) in self.bindings.iter().enumerate() {
            if matches!(binding.key, Key::Letter(c) if !c.is_ascii_lowercase())
                || matches!(binding.key, Key::Digit(c) if !c.is_ascii_digit())
                || matches!(binding.key, Key::Function(n) if !(1..=24).contains(&n))
                || matches!(binding.key, Key::Character(c) if !c.is_ascii_punctuation() || matches!(c, '+' | '=' | '-'))
            {
                return Err("Use lowercase ASCII letters, digits, punctuation or F1–F24".to_owned());
            }
            if self.bindings[..i].iter().any(|other| {
                other.key == binding.key
                    && other.modifiers.primary == binding.modifiers.primary
                    && other.modifiers.secondary == binding.modifiers.secondary
                    && other.modifiers.alt == binding.modifiers.alt
                    && (other.optional_shift
                        || binding.optional_shift
                        || other.modifiers.shift == binding.modifiers.shift)
            }) {
                return Err(format!(
                    "Conflicting shortcut: {} is already assigned to {}",
                    binding.label(cfg!(target_os = "macos")),
                    self.bindings[..i]
                        .iter()
                        .find(|other| other.matches(&binding.key, binding.modifiers)
                            || binding.matches(&other.key, other.modifiers))
                        .unwrap()
                        .action
                        .title()
                ));
            }
        }
        Ok(())
    }

    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        if path.exists() {
            let mut map: Self =
                serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Invalid shortcuts.json: {e}"))?;
            map.validate()?;
            // An explicitly empty map disables every shortcut, including in
            // files predating per-action defaults and the v8/v9 additions.
            if map.bindings.is_empty() {
                if map.version < 9 {
                    map.version = 9;
                    map.save(path)?;
                }
                return Ok(map);
            }
            // Upgrade only exact shipped defaults. User-edited maps are preserved.
            let mut v4 = Self::default();
            v4.bindings.retain(|b| {
                !Action::PRACTICE.contains(&b.action) && b.action != Action::PresentationMode
            });
            v4.bindings.retain(|b| {
                !matches!(
                    b.action,
                    Action::ShuffleAll
                        | Action::ToggleTranslation
                        | Action::DeviceInformation
                        | Action::TrackDetails
                )
            });
            let mut v3 = v4.clone();
            v3.bindings.retain(|b| b.action != Action::JumpToLetter);
            let mut v2 = v3.clone();
            for binding in &mut v2.bindings {
                if binding.key == Key::Letter('h') && !binding.modifiers.shift {
                    binding.action = Action::Back;
                }
                if binding.key == Key::Letter('l') {
                    binding.action = Action::Activate;
                }
            }
            let mut legacy = v2.clone();
            legacy.bindings.retain(|b| {
                b.action != Action::Settings && !(b.action == Action::Home && b.modifiers.shift)
            });
            for binding in &mut legacy.bindings {
                if binding.key == Key::Letter('h') {
                    binding.action = Action::Home;
                }
                if binding.key == Key::Letter('l') {
                    binding.action = Action::ToggleLyrics;
                }
            }
            legacy.bindings.retain(|b| b.key != Key::Letter('y'));
            if (map.version == 1
                && map.bindings.len() == legacy.bindings.len()
                && map.bindings.iter().all(|b| legacy.bindings.contains(b)))
                || (map.version == 2
                    && map.bindings.len() == v2.bindings.len()
                    && map.bindings.iter().all(|b| v2.bindings.contains(b)))
                || (map.version == 3
                    && map.bindings.len() == v3.bindings.len()
                    && map.bindings.iter().all(|b| v3.bindings.contains(b)))
            {
                map = Self::default();
                map.save(path)?;
            }
            if map.version < 5 {
                // Add the new command without replacing aliases or stealing a custom chord.
                let shortcut = Self::default()
                    .bindings
                    .into_iter()
                    .find(|binding| binding.action == Action::ShuffleAll)
                    .unwrap();
                if !map
                    .bindings
                    .iter()
                    .any(|binding| binding.action == Action::ShuffleAll)
                    && !map
                        .bindings
                        .iter()
                        .any(|binding| binding.matches(&shortcut.key, shortcut.modifiers))
                {
                    map.bindings.push(shortcut);
                }
                map.version = 5;
                map.save(path)?;
            }
            if map.version < 6 {
                let shortcut = Self::default()
                    .bindings
                    .into_iter()
                    .find(|b| b.action == Action::ToggleTranslation)
                    .unwrap();
                if !map.bindings.iter().any(|b| {
                    b.action == Action::ToggleTranslation
                        || b.matches(&shortcut.key, shortcut.modifiers)
                }) {
                    map.bindings.push(shortcut);
                }
                map.version = 6;
                map.save(path)?;
            }
            if map.version < 7 {
                // An empty map explicitly disables every shortcut. Preserve it,
                // and never steal a custom chord or add a second binding for a
                // new command that the user has already assigned elsewhere.
                if !map.bindings.is_empty() {
                    for action in [Action::DeviceInformation, Action::TrackDetails] {
                        let shortcut = Self::default()
                            .bindings
                            .into_iter()
                            .find(|binding| binding.action == action)
                            .unwrap();
                        if !map.bindings.iter().any(|binding| {
                            binding.action == action
                                || binding.matches(&shortcut.key, shortcut.modifiers)
                        }) {
                            map.bindings.push(shortcut);
                        }
                    }
                }
                map.version = 7;
                map.save(path)?;
            }
            if map.version < 8 {
                if !map.bindings.is_empty() {
                    for mut shortcut in Self::default()
                        .bindings
                        .into_iter()
                        .filter(|binding| Action::PRACTICE.contains(&binding.action))
                    {
                        // Reconstruct the actual v8 default before the v9
                        // migration decides whether P is free to replace it.
                        if shortcut.action == Action::PracticeMode {
                            shortcut.modifiers = Modifiers {
                                primary: true,
                                alt: true,
                                ..Modifiers::default()
                            };
                        }
                        // Preserve custom assignments and optional-Shift chords.
                        if !map.bindings.iter().any(|binding| {
                            binding.action == shortcut.action
                                || binding.matches(&shortcut.key, shortcut.modifiers)
                                || shortcut.matches(&binding.key, binding.modifiers)
                        }) {
                            map.bindings.push(shortcut);
                        }
                    }
                }
                map.version = 8;
                map.save(path)?;
            }
            if map.version < 9 {
                let defaults = Self::default();
                let practice = defaults
                    .bindings
                    .iter()
                    .find(|binding| binding.action == Action::PracticeMode)
                    .unwrap();
                let mut previous_practice = practice.clone();
                previous_practice.modifiers = Modifiers {
                    primary: true,
                    alt: true,
                    ..Modifiers::default()
                };
                // Absence in v8 is an explicit unassignment. A custom alias
                // also belongs to the user; migrate only the lone old default.
                if let Some(index) = map
                    .bindings
                    .iter()
                    .position(|binding| binding == &previous_practice)
                    && !map.bindings.iter().any(|binding| {
                        binding.action == Action::PracticeMode && binding != &previous_practice
                    })
                    && !map.bindings.iter().any(|binding| {
                        binding.matches(&practice.key, practice.modifiers)
                            || practice.matches(&binding.key, binding.modifiers)
                    })
                {
                    map.bindings[index] = practice.clone();
                }
                let presentation = defaults
                    .bindings
                    .into_iter()
                    .find(|binding| binding.action == Action::PresentationMode)
                    .unwrap();
                if !map.bindings.iter().any(|binding| {
                    binding.action == Action::PresentationMode
                        || binding.matches(&presentation.key, presentation.modifiers)
                        || presentation.matches(&binding.key, binding.modifiers)
                }) {
                    map.bindings.push(presentation);
                }
                map.version = 9;
                map.save(path)?;
            }
            Ok(map)
        } else {
            let map = Self::default();
            map.save(path)?;
            Ok(map)
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(temporary, path).map_err(|e| e.to_string())
    }

    pub fn edit(
        &self,
        action: Action,
        binding: Option<Binding>,
        replace: bool,
    ) -> Result<Self, String> {
        let mut map = self.clone();
        map.version = 9;
        if replace {
            map.bindings.retain(|b| b.action != action);
        }
        if let Some(binding) = binding {
            map.bindings.push(Binding { action, ..binding });
        }
        map.validate()?;
        Ok(map)
    }

    pub fn restore_action(&self, action: Action) -> Result<Self, String> {
        let mut map = self.edit(action, None, true)?;
        map.bindings.extend(
            Self::default()
                .bindings
                .into_iter()
                .filter(|b| b.action == action),
        );
        map.validate()?;
        Ok(map)
    }

    pub fn labels(&self, action: Action) -> String {
        let labels: Vec<_> = self
            .bindings
            .iter()
            .filter(|b| b.action == action)
            .map(|b| b.label(cfg!(target_os = "macos")))
            .collect();
        if labels.is_empty() {
            "Unassigned".into()
        } else {
            labels.join(" / ")
        }
    }
}

impl Key {
    pub fn from_slint(text: &str) -> Option<Self> {
        use slint::platform::Key as S;
        let c = text.chars().next()?;
        if text.chars().count() != 1 {
            return None;
        }
        Some(if c == char::from(S::UpArrow) {
            Self::Up
        } else if c == char::from(S::DownArrow) {
            Self::Down
        } else if c == char::from(S::LeftArrow) {
            Self::Left
        } else if c == char::from(S::RightArrow) {
            Self::Right
        } else if c == char::from(S::Return) || c == '\n' {
            Self::Enter
        } else if c == char::from(S::Escape) {
            Self::Escape
        } else if c == char::from(S::Backspace) {
            Self::Backspace
        } else if c == char::from(S::Tab) {
            Self::Tab
        } else if c == char::from(S::Delete) {
            Self::Delete
        } else if c == char::from(S::Home) {
            Self::Home
        } else if c == char::from(S::End) {
            Self::End
        } else if c == char::from(S::PageUp) {
            Self::PageUp
        } else if c == char::from(S::PageDown) {
            Self::PageDown
        } else if (char::from(S::F1)..=char::from(S::F24)).contains(&c) {
            Self::Function((c as u32 - char::from(S::F1) as u32 + 1) as u8)
        } else {
            match c {
                ' ' => Self::Space,
                '+' => Self::Plus,
                '=' => Self::Equals,
                '-' => Self::Minus,
                c if c.is_ascii_alphabetic() => Self::Letter(c.to_ascii_lowercase()),
                c if c.is_ascii_digit() => Self::Digit(c),
                c if c.is_ascii_punctuation() => Self::Character(c),
                _ => return None,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::action::Category,
        input::{Context, Dispatch, Dispatcher, KeyPress, Surface},
    };

    fn context(surface: Surface) -> Context {
        Context {
            surface,
            text_input: false,
            reference_open: false,
            native_dialog: false,
        }
    }

    fn press(key: char, repeat: bool) -> KeyPress {
        KeyPress {
            key: Key::Letter(key),
            modifiers: Modifiers::default(),
            repeat,
        }
    }

    fn binding(action: Action, key: char) -> Binding {
        Binding {
            action,
            key: Key::Letter(key),
            modifiers: Modifiers::default(),
            optional_shift: false,
        }
    }

    fn v8_defaults() -> Keymap {
        let mut map = Keymap {
            version: 8,
            ..Keymap::default()
        };
        map.bindings
            .retain(|binding| binding.action != Action::PresentationMode);
        let practice = map
            .bindings
            .iter_mut()
            .find(|binding| binding.action == Action::PracticeMode)
            .unwrap();
        practice.modifiers = Modifiers {
            primary: true,
            alt: true,
            ..Modifiers::default()
        };
        map
    }

    fn v7_defaults() -> Keymap {
        let mut map = Keymap {
            version: 7,
            ..v8_defaults()
        };
        map.bindings
            .retain(|binding| !Action::PRACTICE.contains(&binding.action));
        map
    }

    fn without_practice_or_presentation(map: &Keymap) -> Vec<Binding> {
        map.bindings
            .iter()
            .filter(|binding| {
                !Action::PRACTICE.contains(&binding.action)
                    && binding.action != Action::PresentationMode
            })
            .cloned()
            .collect()
    }

    fn v6_defaults() -> Keymap {
        let mut map = Keymap {
            version: 6,
            ..v7_defaults()
        };
        map.bindings.retain(|binding| {
            !matches!(
                binding.action,
                Action::DeviceInformation | Action::TrackDetails
            )
        });
        map
    }

    #[test]
    fn practice_shortcuts_are_configurable_nonrepeating_playback_actions() {
        let map = Keymap::default();
        map.validate().unwrap();
        let dispatcher = Dispatcher::new(map);
        for (action, serialized) in [
            (Action::PracticeMode, "practice_mode"),
            (Action::PracticeAddMarker, "practice_add_marker"),
            (Action::PracticePreviousMarker, "practice_previous_marker"),
            (Action::PracticeNextMarker, "practice_next_marker"),
            (Action::PracticeRestart, "practice_restart"),
        ] {
            assert_eq!(action.category(), Category::Playback);
            assert!(!action.repeatable());
            assert!(Action::CUSTOMIZABLE.contains(&action));
            assert_eq!(Action::parse(serialized), Some(action));
            let binding = dispatcher
                .keymap
                .bindings
                .iter()
                .find(|b| b.action == action)
                .unwrap();
            if action == Action::PracticeMode {
                assert_eq!(binding.modifiers, Modifiers::default());
            } else {
                assert!(binding.modifiers.primary && binding.modifiers.alt);
            }
            let press = KeyPress {
                key: binding.key.clone(),
                modifiers: binding.modifiers,
                repeat: false,
            };
            for surface in [Surface::Home, Surface::NowPlaying, Surface::Library] {
                assert_eq!(
                    dispatcher.dispatch(&press, context(surface)),
                    Dispatch::Action(action)
                );
                assert_eq!(
                    dispatcher.dispatch(
                        &KeyPress {
                            repeat: true,
                            ..press.clone()
                        },
                        context(surface)
                    ),
                    Dispatch::Consumed
                );
                assert_eq!(
                    dispatcher.dispatch(
                        &press,
                        Context {
                            text_input: true,
                            ..context(surface)
                        }
                    ),
                    Dispatch::Ignored
                );
                assert_eq!(
                    dispatcher.dispatch(
                        &press,
                        Context {
                            native_dialog: true,
                            ..context(surface)
                        }
                    ),
                    Dispatch::Ignored
                );
            }
        }
    }

    #[test]
    fn version_seven_adds_practice_defaults_once_without_replacing_the_map() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        let old = v7_defaults();
        old.save(&path).unwrap();
        let migrated = Keymap::load_or_create(&path).unwrap();
        assert_eq!(migrated.version, Keymap::default().version);
        assert_eq!(&migrated.bindings[..old.bindings.len()], old.bindings);
        assert_eq!(
            migrated.bindings.len(),
            old.bindings.len() + Action::PRACTICE.len() + 1
        );
        assert_eq!(migrated.bindings, Keymap::default().bindings);
        assert_eq!(
            Keymap::load_or_create(&path).unwrap().bindings,
            migrated.bindings
        );
    }

    #[test]
    fn practice_migration_preserves_custom_assignments_and_optional_shift_conflicts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        let mut old = v7_defaults();
        let mut conflict = binding(Action::NextTrack, 'p');
        conflict.modifiers.primary = true;
        conflict.modifiers.alt = true;
        conflict.modifiers.shift = true;
        conflict.optional_shift = true;
        old.bindings.push(conflict);
        let mut custom = binding(Action::PracticeAddMarker, 'q');
        custom.modifiers.primary = true;
        old.bindings.push(custom.clone());
        old.save(&path).unwrap();
        let migrated = Keymap::load_or_create(&path).unwrap();
        assert_eq!(&migrated.bindings[..old.bindings.len()], old.bindings);
        assert!(
            !migrated
                .bindings
                .iter()
                .any(|binding| binding.action == Action::PracticeMode)
        );
        assert_eq!(
            migrated
                .bindings
                .iter()
                .filter(|binding| binding.action == Action::PracticeAddMarker)
                .collect::<Vec<_>>(),
            [&custom]
        );
        assert_eq!(migrated.bindings.len(), old.bindings.len() + 4);
        migrated.validate().unwrap();
    }

    #[test]
    fn p_and_q_match_only_plain_keys_and_leave_space_unchanged() {
        let dispatcher = Dispatcher::new(Keymap::default());
        for (key, action) in [('p', Action::PracticeMode), ('q', Action::PresentationMode)] {
            for mask in 0..16 {
                let modifiers = Modifiers {
                    primary: mask & 1 != 0,
                    secondary: mask & 2 != 0,
                    alt: mask & 4 != 0,
                    shift: mask & 8 != 0,
                };
                assert_eq!(
                    dispatcher.dispatch(
                        &KeyPress {
                            modifiers,
                            ..press(key, false)
                        },
                        context(Surface::NowPlaying)
                    ),
                    if mask == 0 {
                        Dispatch::Action(action)
                    } else {
                        Dispatch::Ignored
                    },
                    "{key} with {modifiers:?}",
                );
            }
            assert_eq!(
                dispatcher.dispatch(&press(key, true), context(Surface::NowPlaying)),
                Dispatch::Consumed
            );
            for ctx in [
                Context {
                    text_input: true,
                    ..context(Surface::NowPlaying)
                },
                Context {
                    native_dialog: true,
                    ..context(Surface::NowPlaying)
                },
            ] {
                assert_eq!(
                    dispatcher.dispatch(&press(key, false), ctx),
                    Dispatch::Ignored
                );
            }
        }
        assert_eq!(
            dispatcher.dispatch(
                &KeyPress {
                    key: Key::Space,
                    modifiers: Modifiers::default(),
                    repeat: false
                },
                context(Surface::NowPlaying)
            ),
            Dispatch::Action(Action::TogglePlay)
        );
    }

    #[test]
    fn version_eight_replaces_only_the_old_practice_default_and_adds_q_once() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        let old = v8_defaults();
        old.save(&path).unwrap();
        let migrated = Keymap::load_or_create(&path).unwrap();
        assert_eq!(migrated.version, 9);
        assert_eq!(migrated.bindings, Keymap::default().bindings);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            Keymap::load_or_create(&path).unwrap().bindings,
            migrated.bindings
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn version_eight_preserves_custom_practice_bindings_and_explicit_unassignment() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        for retain_old_default in [false, true] {
            let mut map = v8_defaults();
            if !retain_old_default {
                map.bindings
                    .retain(|binding| binding.action != Action::PracticeMode);
            }
            let custom = binding(Action::PracticeMode, 'u');
            map.bindings.push(custom);
            let previous: Vec<_> = map
                .bindings
                .iter()
                .filter(|b| b.action == Action::PracticeMode)
                .cloned()
                .collect();
            map.save(&path).unwrap();
            let migrated = Keymap::load_or_create(&path).unwrap();
            let actual: Vec<_> = migrated
                .bindings
                .iter()
                .filter(|b| b.action == Action::PracticeMode)
                .cloned()
                .collect();
            assert_eq!(actual, previous);
            assert!(
                !actual
                    .iter()
                    .any(|b| b.key == Key::Letter('p') && b.modifiers == Modifiers::default())
            );
        }
        let mut unassigned = v8_defaults();
        unassigned
            .bindings
            .retain(|binding| binding.action != Action::PracticeMode);
        unassigned.save(&path).unwrap();
        let migrated = Keymap::load_or_create(&path).unwrap();
        assert!(
            !migrated
                .bindings
                .iter()
                .any(|binding| binding.action == Action::PracticeMode)
        );
        assert!(
            migrated
                .bindings
                .contains(&binding(Action::PresentationMode, 'q'))
        );
    }

    #[test]
    fn p_conflicts_preserve_the_old_entry_and_distinct_shift_chords_do_not_block_migration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        for (shift, optional_shift, should_replace) in [
            (false, false, false),
            (true, true, false),
            (true, false, true),
        ] {
            let mut map = v8_defaults();
            let old = map
                .bindings
                .iter()
                .find(|b| b.action == Action::PracticeMode)
                .unwrap()
                .clone();
            let mut occupied = binding(Action::NextTrack, 'p');
            occupied.modifiers.shift = shift;
            occupied.optional_shift = optional_shift;
            map.bindings.push(occupied.clone());
            map.save(&path).unwrap();
            let migrated = Keymap::load_or_create(&path).unwrap();
            assert!(migrated.bindings.contains(&occupied));
            assert_eq!(migrated.bindings.contains(&old), !should_replace);
            assert_eq!(
                migrated
                    .bindings
                    .contains(&binding(Action::PracticeMode, 'p')),
                should_replace
            );
            migrated.validate().unwrap();
        }
    }

    #[test]
    fn q_migration_preserves_existing_actions_aliases_and_optional_shift_chords() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        for existing in [
            binding(Action::NextTrack, 'q'),
            Binding {
                modifiers: Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
                optional_shift: true,
                ..binding(Action::NextTrack, 'q')
            },
            binding(Action::PresentationMode, 'u'),
        ] {
            let mut map = v8_defaults();
            map.bindings.push(existing.clone());
            map.save(&path).unwrap();
            let migrated = Keymap::load_or_create(&path).unwrap();
            assert!(migrated.bindings.contains(&existing));
            assert!(
                !migrated
                    .bindings
                    .contains(&binding(Action::PresentationMode, 'q'))
            );
            migrated.validate().unwrap();
        }
        let mut disabled = Keymap::default();
        disabled.bindings.retain(|binding| {
            !matches!(
                binding.action,
                Action::PracticeMode | Action::PresentationMode
            )
        });
        disabled.save(&path).unwrap();
        assert_eq!(
            Keymap::load_or_create(&path).unwrap().bindings,
            disabled.bindings
        );
    }

    #[test]
    fn desktop_information_actions_use_plain_keys_once_through_the_dispatcher() {
        let dispatcher = Dispatcher::new(Keymap::default());
        for surface in [
            Surface::Home,
            Surface::Library,
            Surface::NowPlaying,
            Surface::NowPlayingVolume,
            Surface::VideoPlaying,
            Surface::Placeholder,
        ] {
            for (key, action) in [
                ('b', Action::DeviceInformation),
                ('d', Action::TrackDetails),
            ] {
                assert_eq!(
                    dispatcher.dispatch(&press(key, false), context(surface)),
                    Dispatch::Action(action)
                );
                assert_eq!(
                    dispatcher.dispatch(&press(key, true), context(surface)),
                    Dispatch::Consumed
                );
            }
        }
        for (action, title, serialized) in [
            (
                Action::DeviceInformation,
                "Device Information",
                "device_information",
            ),
            (Action::TrackDetails, "Track Details", "track_details"),
        ] {
            assert_eq!(action.title(), title);
            assert_eq!(action.category(), Category::Navigation);
            assert!(!action.repeatable());
            assert!(Action::CUSTOMIZABLE.contains(&action));
            assert_eq!(Action::parse(serialized), Some(action));
        }
    }

    #[test]
    fn information_shortcuts_respect_text_input_native_dialogs_and_reference_modal() {
        let dispatcher = Dispatcher::new(Keymap::default());
        for key in ['b', 'd'] {
            for repeat in [false, true] {
                assert_eq!(
                    dispatcher.dispatch(
                        &press(key, repeat),
                        Context {
                            text_input: true,
                            ..context(Surface::Library)
                        },
                    ),
                    Dispatch::Ignored
                );
                assert_eq!(
                    dispatcher.dispatch(
                        &press(key, repeat),
                        Context {
                            native_dialog: true,
                            ..context(Surface::NowPlaying)
                        },
                    ),
                    Dispatch::Ignored
                );
                assert_eq!(
                    dispatcher.dispatch(
                        &press(key, repeat),
                        Context {
                            reference_open: true,
                            ..context(Surface::Home)
                        },
                    ),
                    Dispatch::Consumed
                );
            }
        }
    }

    #[test]
    fn information_shortcuts_do_not_capture_modified_b_or_d() {
        let dispatcher = Dispatcher::new(Keymap::default());
        for key in ['b', 'd'] {
            for modifiers in [
                Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
                Modifiers {
                    primary: true,
                    ..Modifiers::default()
                },
                Modifiers {
                    secondary: true,
                    ..Modifiers::default()
                },
                Modifiers {
                    alt: true,
                    ..Modifiers::default()
                },
            ] {
                assert_eq!(
                    dispatcher.dispatch(
                        &KeyPress {
                            modifiers,
                            ..press(key, false)
                        },
                        context(Surface::NowPlaying),
                    ),
                    Dispatch::Ignored
                );
            }
        }
    }

    #[test]
    fn version_six_migration_adds_only_missing_information_defaults_and_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        let old = v6_defaults();
        old.save(&path).unwrap();
        let migrated = Keymap::load_or_create(&path).unwrap();
        assert_eq!(migrated.version, Keymap::default().version);
        assert_eq!(&migrated.bindings[..old.bindings.len()], old.bindings);
        assert_eq!(migrated.bindings.len(), old.bindings.len() + 8);
        for (key, action) in [
            ('b', Action::DeviceInformation),
            ('d', Action::TrackDetails),
        ] {
            assert!(migrated.bindings.contains(&binding(action, key)));
        }
        assert_eq!(
            Keymap::load_or_create(&path).unwrap().bindings,
            migrated.bindings
        );
    }

    #[test]
    fn migration_preserves_custom_chords_and_existing_information_assignments() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        let mut custom_information = binding(Action::TrackDetails, 'q');
        custom_information.modifiers.primary = true;
        let original = vec![binding(Action::NextTrack, 'b'), custom_information];
        Keymap {
            version: 6,
            bindings: original.clone(),
        }
        .save(&path)
        .unwrap();
        let migrated = Keymap::load_or_create(&path).unwrap();
        assert_eq!(migrated.version, Keymap::default().version);
        assert_eq!(without_practice_or_presentation(&migrated), original);
    }

    #[test]
    fn migration_checks_optional_shift_conflicts_without_taking_distinct_shifted_chords() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        let mut optional_b = binding(Action::NextTrack, 'b');
        optional_b.modifiers.shift = true;
        optional_b.optional_shift = true;
        let mut shifted_d = binding(Action::PreviousTrack, 'd');
        shifted_d.modifiers.shift = true;
        let old = vec![optional_b, shifted_d];
        Keymap {
            version: 6,
            bindings: old.clone(),
        }
        .save(&path)
        .unwrap();
        let migrated = Keymap::load_or_create(&path).unwrap();
        assert_eq!(&migrated.bindings[..old.len()], old);
        assert!(
            !migrated
                .bindings
                .iter()
                .any(|binding| binding.action == Action::DeviceInformation)
        );
        assert_eq!(
            without_practice_or_presentation(&migrated).last(),
            Some(&binding(Action::TrackDetails, 'd'))
        );
    }

    #[test]
    fn explicit_empty_maps_and_unassigned_actions_stay_disabled() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        for version in 1..=9 {
            Keymap {
                version,
                bindings: Vec::new(),
            }
            .save(&path)
            .unwrap();
            let loaded = Keymap::load_or_create(&path).unwrap();
            assert_eq!(loaded.version, Keymap::default().version);
            assert!(loaded.bindings.is_empty());
        }
        let unassigned = Keymap::default()
            .edit(Action::DeviceInformation, None, true)
            .unwrap()
            .edit(Action::TrackDetails, None, true)
            .unwrap();
        unassigned.save(&path).unwrap();
        assert_eq!(
            Keymap::load_or_create(&path).unwrap().bindings,
            unassigned.bindings
        );
    }

    #[test]
    fn remapped_information_actions_use_the_existing_shortcut_editor_contract() {
        let remapped = Keymap::default()
            .edit(
                Action::DeviceInformation,
                Some(binding(Action::DeviceInformation, 'u')),
                true,
            )
            .unwrap();
        assert_eq!(remapped.version, Keymap::default().version);
        let dispatcher = Dispatcher::new(remapped);
        assert_eq!(
            dispatcher.dispatch(&press('b', false), context(Surface::Home)),
            Dispatch::Ignored
        );
        assert_eq!(
            dispatcher.dispatch(&press('u', false), context(Surface::Home)),
            Dispatch::Action(Action::DeviceInformation)
        );
        let restored = dispatcher
            .keymap
            .restore_action(Action::DeviceInformation)
            .unwrap();
        assert_eq!(restored.labels(Action::DeviceInformation), "B");
        restored.validate().unwrap();
    }

    #[test]
    fn genuine_shipped_version_one_defaults_still_upgrade_navigation_and_new_actions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("shortcuts.json");
        let mut legacy = v6_defaults();
        legacy.version = 1;
        legacy.bindings.retain(|binding| {
            !matches!(
                binding.action,
                Action::ShuffleAll
                    | Action::ToggleTranslation
                    | Action::Settings
                    | Action::JumpToLetter
            ) && !(binding.action == Action::Home && binding.modifiers.shift)
                && binding.key != Key::Letter('y')
        });
        for binding in &mut legacy.bindings {
            if binding.key == Key::Letter('h') {
                binding.action = Action::Home;
            }
            if binding.key == Key::Letter('l') {
                binding.action = Action::ToggleLyrics;
            }
        }
        legacy.save(&path).unwrap();
        assert_eq!(
            Keymap::load_or_create(&path).unwrap().bindings,
            Keymap::default().bindings
        );
    }

    #[test]
    fn adding_information_shortcuts_preserves_existing_r_dispatch() {
        let dispatcher = Dispatcher::new(Keymap::default());
        assert_eq!(
            dispatcher.dispatch(&press('r', false), context(Surface::Home)),
            Dispatch::Action(Action::CycleRepeat)
        );
        assert_eq!(
            dispatcher.dispatch(&press('r', false), context(Surface::NowPlaying)),
            Dispatch::Action(Action::ToggleRomaji)
        );
        let remapped = dispatcher
            .keymap
            .edit(
                Action::CycleRepeat,
                Some(binding(Action::CycleRepeat, 'u')),
                true,
            )
            .unwrap();
        assert_eq!(
            Dispatcher::new(remapped).dispatch(&press('u', false), context(Surface::NowPlaying)),
            Dispatch::Action(Action::CycleRepeat)
        );
    }
}

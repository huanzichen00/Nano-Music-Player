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
            version: 6,
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
                .collect(),
        }
    }
}

impl Keymap {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.version, 1..=6) {
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
            // Upgrade only exact shipped defaults. User-edited maps are preserved.
            let mut v4 = Self::default();
            v4.bindings
                .retain(|b| !matches!(b.action, Action::ShuffleAll | Action::ToggleTranslation));
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
        map.version = 6;
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

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
            Activate, Back, CycleRepeat, DeviceMode, Home, ImportFolder, NextTrack, NowPlaying,
            PreviousTrack, ScreenMode, Search, SelectNext, SelectPrevious, ToggleLyrics,
            ToggleMute, TogglePlay, ToggleShuffle, VolumeDown, VolumeUp, ZoomIn, ZoomOut,
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
            (Letter('h'), Home),
            (Left, Action::Left),
            (Right, Action::Right),
            (Space, TogglePlay),
            (Letter('m'), ToggleMute),
            (Letter('s'), ToggleShuffle),
            (Letter('r'), CycleRepeat),
            (Letter('n'), NowPlaying),
            (Letter('l'), ToggleLyrics),
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
        ];
        Self {
            version: 1,
            bindings: plain
                .into_iter()
                .map(|(key, action)| Binding {
                    action,
                    key,
                    modifiers: Modifiers::default(),
                    optional_shift: false,
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
                .collect(),
        }
    }
}

impl Keymap {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!(
                "Unsupported shortcut file version: {}",
                self.version
            ));
        }
        for (i, binding) in self.bindings.iter().enumerate() {
            if matches!(binding.key, Key::Letter(c) if !c.is_ascii_lowercase())
                || matches!(binding.key, Key::Digit(c) if !c.is_ascii_digit())
            {
                return Err("Use lowercase ASCII letters or digits for key bindings".to_owned());
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
                    "Conflicting shortcut: {}",
                    binding.label(cfg!(target_os = "macos"))
                ));
            }
        }
        Ok(())
    }

    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        if path.exists() {
            let map: Self =
                serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("Invalid shortcuts.json: {e}"))?;
            map.validate()?;
            Ok(map)
        } else {
            let map = Self::default();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(
                path,
                serde_json::to_vec_pretty(&map).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            Ok(map)
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
        } else {
            match c {
                ' ' => Self::Space,
                '+' => Self::Plus,
                '=' => Self::Equals,
                '-' => Self::Minus,
                c if c.is_ascii_alphabetic() => Self::Letter(c.to_ascii_lowercase()),
                c if c.is_ascii_digit() => Self::Digit(c),
                _ => return None,
            }
        })
    }
}

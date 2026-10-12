use super::{Key, Keymap, Modifiers};
use crate::app::action::{Action, Category};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Home,
    Library,
    NowPlaying,
    NowPlayingVolume,
    Placeholder,
}

#[derive(Debug, Clone, Copy)]
pub struct Context {
    pub surface: Surface,
    pub text_input: bool,
    pub reference_open: bool,
    pub native_dialog: bool,
}

#[derive(Debug, Clone)]
pub struct KeyPress {
    pub key: Key,
    pub modifiers: Modifiers,
    pub repeat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    Ignored,
    Consumed,
    Action(Action),
}

pub struct Dispatcher {
    pub keymap: Keymap,
}

impl Dispatcher {
    pub fn new(keymap: Keymap) -> Self {
        Self { keymap }
    }

    /// The only shortcut resolver. Text editing and modal precedence are decided before binding lookup.
    pub fn dispatch(&self, press: &KeyPress, context: Context) -> Dispatch {
        use Action::*;
        if context.native_dialog {
            return Dispatch::Ignored;
        }
        if context.text_input {
            // Search lifecycle controls, not application shortcuts. All editing keys reach TextInput.
            if press.modifiers == Modifiers::default()
                && matches!(press.key, Key::Escape | Key::Enter)
            {
                return if press.repeat {
                    Dispatch::Consumed
                } else {
                    Dispatch::Action(FinishSearchEditing)
                };
            }
            return Dispatch::Ignored;
        }
        if context.reference_open
            && press.key == Key::Escape
            && press.modifiers == Modifiers::default()
        {
            return if press.repeat {
                Dispatch::Consumed
            } else {
                Dispatch::Action(CloseShortcuts)
            };
        }
        let Some(binding) = self
            .keymap
            .bindings
            .iter()
            .find(|b| b.matches(&press.key, press.modifiers))
        else {
            return Dispatch::Ignored;
        };
        if press.repeat && !binding.action.repeatable() {
            return Dispatch::Consumed;
        }
        if context.reference_open && binding.action.category() == Category::Navigation {
            return Dispatch::Consumed;
        }
        let action = match (binding.action, context.surface) {
            // Plain R uses the existing repeat binding outside the song interface.
            // Customized bindings still win, and a remapped Repeat key stays Repeat.
            (CycleRepeat, Surface::NowPlaying | Surface::NowPlayingVolume)
                if press.key == Key::Letter('r') && press.modifiers == Modifiers::default() =>
            {
                ToggleRomaji
            }
            (ToggleRomaji, Surface::Home | Surface::Library | Surface::Placeholder) => {
                return Dispatch::Consumed;
            }
            (NavigateLeft, Surface::Home) => Left,
            (NavigateRight, Surface::Home) => Right,
            (NavigateLeft, _) => Back,
            (NavigateRight, Surface::Placeholder) => return Dispatch::Consumed,
            (NavigateRight, _) => Activate,
            (Left, Surface::Library | Surface::Placeholder) => Back,
            (Right, Surface::Library) => Activate,
            (Left, Surface::NowPlaying) => SeekBackward,
            (Right, Surface::NowPlaying) => SeekForward,
            (Left, Surface::NowPlayingVolume) => VolumeDown,
            (Right, Surface::NowPlayingVolume) => VolumeUp,
            (Right, Surface::Placeholder) => return Dispatch::Consumed,
            (action, _) => action,
        };
        if context.reference_open && action == ToggleRomaji {
            return Dispatch::Consumed;
        }
        // A held Right arrow in a list must not open several nested screens.
        if press.repeat && !action.repeatable() {
            Dispatch::Consumed
        } else {
            Dispatch::Action(action)
        }
    }
}

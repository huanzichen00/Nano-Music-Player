use serde::{Deserialize, Serialize};

/// Stable persisted choices; bundled fonts work without installing them system-wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Font {
    #[default]
    Classic,
    MapleMono,
    JetBrainsMono,
}

impl Font {
    pub const ALL: [Self; 3] = [Self::Classic, Self::MapleMono, Self::JetBrainsMono];

    pub const fn family(self) -> &'static str {
        match self {
            Self::Classic => "Helvetica Neue",
            Self::MapleMono => "Maple Mono",
            Self::JetBrainsMono => "JetBrains Mono",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::MapleMono => "Maple Mono",
            Self::JetBrainsMono => "JetBrains Mono",
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Classic => 0,
            Self::MapleMono => 1,
            Self::JetBrainsMono => 2,
        }
    }
}

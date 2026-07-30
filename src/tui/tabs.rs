//! TUI tab + pending action types.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    Chats,
    Personas,
    Memory,
    Skills,
    Models,
    Doctor,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Home,
        Tab::Chats,
        Tab::Personas,
        Tab::Memory,
        Tab::Skills,
        Tab::Models,
        Tab::Doctor,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Chats => "Chats",
            Tab::Personas => "Personas",
            Tab::Memory => "Memory",
            Tab::Skills => "Skills",
            Tab::Models => "Models",
            Tab::Doctor => "Doctor",
        }
    }

    pub fn next(self) -> Self {
        let i = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Tab::ALL[(i + 1) % Tab::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let i = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Tab::ALL[(i + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    None,
    RunSession { fresh: bool },
    RunPleaseFix,
    Slash(String),
}

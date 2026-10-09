//! First-run state shared by the Linux startup coordinator and the settings UI.
use crate::{Text, tr};
use okbs_core::Lang;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAccess {
    Ready,
    NeedsSetup,
    NoKeyboard,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAccess {
    Ready,
    MissingLayouts,
    NeedsIntegration,
    Unsupported,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAccess {
    Ready,
    Inactive,
    Root,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Authorize,
    InstallIntegration,
    UseAutoBackend,
    UseAllKeyboards,
    KeyboardSettings,
    Recheck,
    Continue,
    Quit,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub input: InputAccess,
    pub desktop: DesktopAccess,
    pub session: SessionAccess,
    pub busy: bool,
    pub error: Option<String>,
    pub session_restart: bool,
    pub selected_devices: bool,
}
impl Status {
    pub fn ready(&self) -> bool {
        self.input == InputAccess::Ready
            && self.desktop == DesktopAccess::Ready
            && self.session == SessionAccess::Ready
            && !self.busy
    }
    pub fn allows(&self, action: Action) -> bool {
        match action {
            Action::Quit => true,
            Action::Continue => self.ready(),
            Action::Recheck => !self.busy,
            Action::Authorize => {
                !self.busy
                    && self.session == SessionAccess::Ready
                    && self.input == InputAccess::NeedsSetup
            }
            Action::InstallIntegration => {
                !self.busy
                    && self.session != SessionAccess::Root
                    && self.desktop == DesktopAccess::NeedsIntegration
            }
            Action::UseAutoBackend => !self.busy && self.desktop == DesktopAccess::Unsupported,
            Action::UseAllKeyboards => {
                !self.busy && self.selected_devices && self.input == InputAccess::NoKeyboard
            }
            Action::KeyboardSettings => !self.busy && self.desktop != DesktopAccess::Ready,
        }
    }
    pub(crate) fn lines(&self, lang: Lang) -> [&'static str; 3] {
        [
            tr(
                match self.input {
                    InputAccess::Ready => Text::LinuxInputReady,
                    InputAccess::NeedsSetup => Text::LinuxInputSetup,
                    InputAccess::NoKeyboard => Text::LinuxNoKeyboard,
                    InputAccess::Unavailable => Text::LinuxInputUnavailable,
                },
                lang,
            ),
            tr(
                match self.desktop {
                    DesktopAccess::Ready => Text::LinuxDesktopReady,
                    DesktopAccess::MissingLayouts => Text::LinuxMissingLayouts,
                    DesktopAccess::NeedsIntegration => Text::LinuxDesktopSetup,
                    DesktopAccess::Unsupported => Text::LinuxDesktopUnsupported,
                    DesktopAccess::Unavailable => Text::LinuxDesktopUnavailable,
                },
                lang,
            ),
            tr(
                match self.session {
                    SessionAccess::Ready => Text::LinuxSessionReady,
                    SessionAccess::Inactive => Text::LinuxSessionInactive,
                    SessionAccess::Root => Text::LinuxSessionRoot,
                },
                lang,
            ),
        ]
    }
}

//! Tray icon and application menu.
//!
//! Windows uses `Shell_NotifyIcon`, Linux the StatusNotifierItem protocol
//! (through `tray-icon`'s `ksni` backend, without GTK).

use crate::i18n::{Text, tr};
use crate::icon::{self, ICON_SIZE, IconState};
use okbs_core::Lang;
use okbs_core::config::AutoReplace;
use std::time::{Duration, Instant};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// A menu command chosen by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    AutoreplaceMenu,
    AutoreplaceList,
    InsertAutoreplace(usize),
    /// «Настройки...» or a double click on the icon.
    OpenSettings,
    /// «Автопереключение».
    ToggleAutoswitch,
    /// «Звуковые эффекты».
    ToggleSounds,
    /// «Буфер обмена → Изменить раскладку».
    ClipboardLayout,
    /// «Буфер обмена → Транслитерировать».
    ClipboardTransliterate,
    /// «Буфер обмена → Проверить орфографию».
    ClipboardSpellcheck,
    /// «Буфер обмена → Посмотреть историю...».
    ClipboardHistory,
    /// «Выйти».
    Exit,
}

/// Everything the tray shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayState {
    /// Icon contents.
    pub icon: IconState,
    /// Sounds are on.
    pub sounds: bool,
}

/// Error creating or updating the tray icon.
#[derive(Debug, thiserror::Error)]
#[error("tray icon: {0}")]
pub struct TrayError(String);

fn err(e: impl std::fmt::Display) -> TrayError {
    TrayError(e.to_string())
}

/// The tray icon and its menu.
pub struct Tray {
    autoreplace_list: MenuItem,
    autoreplace_items: Vec<MenuItem>,
    left_opens_autoreplace: bool,
    left_click: LeftClick,
    icon: TrayIcon,
    state: TrayState,
    ui: Lang,
    settings: MenuItem,
    autoswitch: CheckMenuItem,
    sounds: CheckMenuItem,
    clipboard_layout: MenuItem,
    clipboard_translit: MenuItem,
    clipboard_spellcheck: MenuItem,
    clipboard_history: MenuItem,
    exit: MenuItem,
}

impl std::fmt::Debug for Tray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tray")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

fn image(state: IconState) -> Result<Icon, TrayError> {
    Icon::from_rgba(icon::render(state), ICON_SIZE, ICON_SIZE).map_err(err)
}

fn tooltip(state: TrayState, ui: Lang) -> String {
    let mut text = format!("{} — {}", tr(Text::AppName, ui), state.icon.label_text());
    if !state.icon.autoswitch {
        text.push_str(match ui {
            Lang::Ru => " (автопереключение выключено)",
            Lang::En => " (auto switch off)",
        });
    }
    text
}

impl Tray {
    /// Creates the tray icon. On Windows call it on a thread that pumps messages.
    pub fn new(state: TrayState, ui: Lang, autoreplace: &AutoReplace) -> Result<Self, TrayError> {
        let settings = MenuItem::new(tr(Text::MenuSettings, ui), true, None);
        let autoswitch = CheckMenuItem::new(
            tr(Text::MenuAutoswitch, ui),
            true,
            state.icon.autoswitch,
            None,
        );
        let sounds = CheckMenuItem::new(tr(Text::MenuSounds, ui), true, state.sounds, None);
        let clipboard_layout = MenuItem::new(tr(Text::MenuClipboardConvertLayout, ui), true, None);
        let clipboard_translit =
            MenuItem::new(tr(Text::MenuClipboardTransliterate, ui), true, None);
        let clipboard_spellcheck = MenuItem::new(tr(Text::MenuClipboardSpellcheck, ui), true, None);
        let clipboard_history =
            MenuItem::new(tr(Text::MenuClipboardHistory, ui), cfg!(windows), None);
        let clipboard = Submenu::with_items(
            tr(Text::MenuClipboard, ui),
            true,
            &[
                &clipboard_layout,
                &clipboard_translit,
                &clipboard_spellcheck,
                &PredefinedMenuItem::separator(),
                &clipboard_history,
            ],
        )
        .map_err(err)?;
        let exit = MenuItem::new(tr(Text::MenuExit, ui), true, None);
        let autoreplace_list =
            MenuItem::new(tr(Text::MenuAutoreplaceList, ui), cfg!(windows), None);
        let autoreplace_menu = Submenu::new(tr(Text::SectionAutoreplace, ui), true);
        autoreplace_menu.append(&autoreplace_list).map_err(err)?;
        let mut autoreplace_items = Vec::new();
        if autoreplace.show_in_tray_menu {
            autoreplace_menu
                .append(&PredefinedMenuItem::separator())
                .map_err(err)?;
            for item in &autoreplace.items {
                let text: String = item
                    .to
                    .chars()
                    .take(45)
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
                let label = format!("{} — {text}", item.from).replace('&', "&&");
                let entry = MenuItem::new(label, autoreplace.enabled && cfg!(windows), None);
                autoreplace_menu.append(&entry).map_err(err)?;
                autoreplace_items.push(entry);
            }
        }
        let menu = Menu::new();
        menu.append_items(&[
            &settings,
            &PredefinedMenuItem::separator(),
            &autoswitch,
            &sounds,
            &PredefinedMenuItem::separator(),
            &clipboard,
            &autoreplace_menu,
            &PredefinedMenuItem::separator(),
            &exit,
        ])
        .map_err(err)?;
        let icon = TrayIconBuilder::new()
            .with_id("okbswitch")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(!cfg!(windows))
            .with_tooltip(tooltip(state, ui))
            .with_icon(image(state.icon)?)
            .build()
            .map_err(err)?;
        Ok(Self {
            autoreplace_list,
            autoreplace_items,
            left_opens_autoreplace: autoreplace.show_in_tray_menu && cfg!(windows),
            left_click: LeftClick::default(),
            icon,
            state,
            ui,
            settings,
            autoswitch,
            sounds,
            clipboard_layout,
            clipboard_translit,
            clipboard_spellcheck,
            clipboard_history,
            exit,
        })
    }

    /// Current state.
    pub fn state(&self) -> TrayState {
        self.state
    }

    /// Updates the icon, tooltip and check marks when something changed.
    pub fn update(&mut self, state: TrayState) -> Result<(), TrayError> {
        if state == self.state {
            return Ok(());
        }
        if state.icon != self.state.icon {
            self.icon.set_icon(Some(image(state.icon)?)).map_err(err)?;
            self.icon
                .set_tooltip(Some(tooltip(state, self.ui)))
                .map_err(err)?;
        }
        self.autoswitch.set_checked(state.icon.autoswitch);
        self.sounds.set_checked(state.sounds);
        self.state = state;
        Ok(())
    }

    /// Interface language of the menu.
    pub fn language(&self) -> Lang {
        self.ui
    }

    /// Next menu command, if any.
    pub fn poll(&mut self) -> Option<TrayCommand> {
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(
                event,
                TrayIconEvent::DoubleClick {
                    button: tray_icon::MouseButton::Left,
                    ..
                }
            ) {
                self.left_click.double_click(Instant::now());
                return Some(TrayCommand::OpenSettings);
            }
            if self.left_opens_autoreplace
                && matches!(
                    event,
                    TrayIconEvent::Click {
                        button: tray_icon::MouseButton::Left,
                        button_state: tray_icon::MouseButtonState::Up,
                        ..
                    }
                )
            {
                self.left_click.release(Instant::now());
            }
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id();
            let command = if id == self.settings.id() {
                TrayCommand::OpenSettings
            } else if id == self.autoswitch.id() {
                TrayCommand::ToggleAutoswitch
            } else if id == self.sounds.id() {
                TrayCommand::ToggleSounds
            } else if id == self.clipboard_layout.id() {
                TrayCommand::ClipboardLayout
            } else if id == self.clipboard_translit.id() {
                TrayCommand::ClipboardTransliterate
            } else if id == self.clipboard_spellcheck.id() {
                TrayCommand::ClipboardSpellcheck
            } else if id == self.clipboard_history.id() {
                TrayCommand::ClipboardHistory
            } else if id == self.autoreplace_list.id() {
                TrayCommand::AutoreplaceList
            } else if let Some(index) = self
                .autoreplace_items
                .iter()
                .position(|item| id == item.id())
            {
                TrayCommand::InsertAutoreplace(index)
            } else if id == self.exit.id() {
                TrayCommand::Exit
            } else {
                continue;
            };
            return Some(command);
        }
        if self.left_click.ready(Instant::now()) {
            return Some(TrayCommand::AutoreplaceMenu);
        }
        None
    }
}

#[derive(Default)]
struct LeftClick {
    pending: Option<Instant>,
    ignore_release_until: Option<Instant>,
}

impl LeftClick {
    const INTERVAL: Duration = Duration::from_millis(500);
    fn release(&mut self, now: Instant) {
        if !self.ignore_release_until.is_some_and(|until| now < until) {
            self.pending = Some(now);
        }
    }
    fn double_click(&mut self, now: Instant) {
        self.pending = None;
        self.ignore_release_until = Some(now + Self::INTERVAL);
    }
    fn ready(&mut self, now: Instant) -> bool {
        if self
            .pending
            .is_some_and(|at| now.saturating_duration_since(at) >= Self::INTERVAL)
        {
            self.pending = None;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_click_does_not_open_the_insert_menu_after_settings() {
        let at = Instant::now();
        let mut click = LeftClick::default();
        click.release(at);
        assert!(!click.ready(at + Duration::from_millis(100)));
        click.double_click(at + Duration::from_millis(150));
        click.release(at + Duration::from_millis(200));
        assert!(!click.ready(at + Duration::from_secs(1)));
        click.release(at + Duration::from_secs(2));
        assert!(click.ready(at + Duration::from_secs(3)));
        assert!(!click.ready(at + Duration::from_secs(4)));
    }
}

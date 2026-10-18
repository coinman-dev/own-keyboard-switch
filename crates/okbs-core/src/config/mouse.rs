//! «Управление мышью»: key combinations sent by extra mouse buttons.

use super::HotkeyBinding;
use crate::hotkey::{Hotkey, Modifiers};
use crate::keys::PhysKey;
use serde::{Deserialize, Serialize};

/// A mouse button that can be assigned a key combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Wheel click.
    Middle,
    /// Rear side button, farther from the wheel (browser «Back»).
    Back,
    /// Front side button, nearer the wheel (browser «Forward»).
    Forward,
}

impl MouseButton {
    /// All buttons in settings-window order.
    pub const ALL: [MouseButton; 3] = [Self::Forward, Self::Back, Self::Middle];
}

/// Управление мышью.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mouse {
    /// Master switch of the assignments below. Off by default: another
    /// mouse program may already own these buttons.
    pub enabled: bool,
    /// Applications do not receive an assigned button's own action.
    pub block_original: bool,
    /// Combination of the front side button; empty keeps its own action.
    pub forward: HotkeyBinding,
    /// Combination of the rear side button.
    pub back: HotkeyBinding,
    /// Combination of the wheel click.
    pub middle: HotkeyBinding,
}

impl Default for Mouse {
    fn default() -> Self {
        Self {
            enabled: false,
            block_original: true,
            forward: HotkeyBinding::some(Hotkey::new(Modifiers::CTRL, PhysKey::KeyC)),
            back: HotkeyBinding::some(Hotkey::new(Modifiers::CTRL, PhysKey::KeyV)),
            middle: HotkeyBinding::NONE,
        }
    }
}

impl Mouse {
    /// Stored assignment of `button`, regardless of the master switch.
    pub fn get(&self, button: MouseButton) -> HotkeyBinding {
        match button {
            MouseButton::Middle => self.middle,
            MouseButton::Back => self.back,
            MouseButton::Forward => self.forward,
        }
    }

    /// Mutable assignment of `button`.
    pub fn get_mut(&mut self, button: MouseButton) -> &mut HotkeyBinding {
        match button {
            MouseButton::Middle => &mut self.middle,
            MouseButton::Back => &mut self.back,
            MouseButton::Forward => &mut self.forward,
        }
    }

    /// Combination to send for `button`, or `None` when it keeps its own action.
    pub fn action(&self, button: MouseButton) -> Option<Hotkey> {
        self.enabled.then(|| self.get(button).0).flatten()
    }

    /// Whether the platform must withhold `button` from applications.
    pub fn blocks(&self, button: MouseButton) -> bool {
        self.block_original && self.action(button).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_and_copy_on_front_and_paste_on_rear() {
        assert!(Mouse::default().action(MouseButton::Back).is_none());
        let mouse = Mouse {
            enabled: true,
            ..Mouse::default()
        };
        assert_eq!(mouse.forward.to_string(), "Ctrl+C");
        assert_eq!(mouse.back.to_string(), "Ctrl+V");
        assert!(mouse.action(MouseButton::Middle).is_none());
        assert!(mouse.blocks(MouseButton::Back));
        assert_eq!(
            mouse
                .action(MouseButton::Forward)
                .map(|h| h.modifier_keys()),
            Some(vec![PhysKey::ControlLeft])
        );
    }

    #[test]
    fn master_switch_disables_actions_and_blocking() {
        let mouse = Mouse {
            enabled: false,
            ..Mouse::default()
        };
        assert!(mouse.action(MouseButton::Forward).is_none());
        assert!(!mouse.blocks(MouseButton::Forward));
    }

    #[test]
    fn partial_file_keeps_defaults() {
        let mouse: Mouse =
            toml::from_str("enabled = true\nmiddle = \"Ctrl+Shift+T\"\nback = \"\"").unwrap();
        assert_eq!(mouse.middle.to_string(), "Ctrl+Shift+T");
        assert!(mouse.back.0.is_none());
        assert_eq!(mouse.forward.to_string(), "Ctrl+C");
    }
}

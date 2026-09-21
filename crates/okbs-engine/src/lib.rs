//! Event processing engine of Own Keyboard Switch.
//!
//! The engine owns the program state and runs on its own thread. Platform
//! backends deliver input through channels; the UI sends [`Command`]s and
//! receives [`Event`]s.

mod processor;
pub mod sounds;

#[cfg(test)]
mod tests;

pub use processor::{Backends, Processor, TextOp, Timing, is_excluded};

use crossbeam_channel::{Receiver, Sender, never, select, unbounded};
use okbs_core::config::{AutoReplaceItem, Config, Rule};
use okbs_core::{Hotkey, Lang};
use okbs_platform::{FocusEvent, InputEvent, InputTarget, LayoutId, LayoutInfo};
use std::thread::JoinHandle;

/// Requests to the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Turns automatic layout switching on or off («Автопереключение»).
    SetAutoswitch(bool),
    /// Flips automatic layout switching.
    ToggleAutoswitch,
    /// Turns all sounds on or off («Звуковые эффекты»).
    SetSounds(bool),
    /// Applies a new configuration.
    ApplyConfig(Box<Config>),
    /// Runs a text operation on the clipboard (tray menu «Буфер обмена»).
    ClipboardOp(TextOp),
    /// Read the clipboard for spelling checks off the input thread.
    SpellcheckClipboard,
    /// Apply background corrections only if the clipboard still has the checked text.
    CorrectClipboard { original: String, corrected: String },
    /// Insert the chosen, still-configured entry into its original application.
    InsertAutoreplace {
        item: AutoReplaceItem,
        target: Option<InputTarget>,
    },
    /// Insert a clipboard history entry into the application it was taken from.
    /// Never log the payload.
    InsertText {
        text: String,
        target: Option<InputTarget>,
    },
    /// Open the insert menu or toggle the floating list.
    AutoreplaceList { toggle: bool },
    /// Reports the next key combination as [`Event::HotkeyCaptured`] instead of acting on it.
    CaptureHotkey,
    /// Leaves the capture mode.
    CancelCapture,
    /// Stops the engine thread.
    Shutdown,
}

/// How a word was converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionKind {
    /// Detected automatically.
    Automatic,
    /// By the hotkey.
    Manual,
    /// An automatic conversion was undone.
    Undo,
}

/// Actions the UI has to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiRequest {
    /// Open the settings window.
    OpenSettings,
    /// Open the autoreplace section.
    OpenAutoreplaceSettings,
    /// Show or hide the autoreplace list.
    ToggleAutoreplaceList,
    /// Show the autoreplace insert menu.
    ShowAutoreplaceMenu,
    /// Add the selected text to autoreplace.
    AddSelectionToAutoreplace,
    /// Spell-check the clipboard.
    SpellcheckClipboard,
    /// Maximize or restore the active window.
    ToggleMaximizeWindow,
    /// Minimize the active window.
    MinimizeWindow,
}

/// Notifications from the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The engine thread started.
    Started,
    /// Automatic switching was turned on or off.
    AutoswitchChanged(bool),
    /// Sounds were turned on or off.
    SoundsChanged(bool),
    /// A configuration was applied.
    ConfigApplied,
    /// The keyboard layout changed; `None` if it is unknown.
    LayoutChanged(Option<LayoutInfo>),
    /// Letter case of the last word was corrected (ПРивет → Привет).
    CaseFixed,
    /// An abbreviation was expanded. The text is deliberately omitted.
    Autoreplaced,
    /// Index of a tooltip candidate, or None to hide it. No typed text is logged.
    AutoreplaceHint(Option<usize>),
    /// Show/toggle the list, retaining the application that should receive input.
    AutoreplaceList {
        toggle: bool,
        target: Option<InputTarget>,
    },
    /// Show the clipboard history, retaining the application that should
    /// receive the entry the user picks.
    ClipboardHistory { target: Option<InputTarget> },
    /// New clipboard text worth remembering. Never log this payload.
    ClipboardText(String),
    /// A word was retyped in another layout.
    Converted {
        /// Layout the word was shown in.
        from: Lang,
        /// Layout it is shown in now.
        to: Lang,
        /// Reason.
        kind: ConversionKind,
    },
    /// The selection was replaced by a converted text.
    SelectionConverted(TextOp),
    /// The clipboard text was converted.
    ClipboardConverted {
        /// Operation.
        op: TextOp,
        /// New clipboard text.
        result: String,
    },
    /// Text to check outside the input thread. Never log this payload.
    CheckSpelling {
        /// Clipboard snapshot.
        text: String,
        /// Options at the time the operation was requested.
        settings: okbs_core::config::Spellcheck,
    },
    /// A word looks like a typo in both layouts.
    Suspicious,
    /// The user undid conversions of a word several times: offer a rule.
    SuggestRule(Rule),
    /// Selected text to prefill an autoreplace entry; never log the payload.
    AutoreplaceSelection(String),
    /// The UI should do something.
    Ui(UiRequest),
    /// A key combination was pressed in capture mode.
    HotkeyCaptured(Hotkey),
    /// Capture mode ended without a combination (Esc).
    CaptureCancelled,
    /// An operation failed.
    Error(String),
    /// The engine thread is about to exit.
    Stopped,
}

/// Channels feeding the engine.
#[derive(Debug)]
pub struct Inputs {
    /// Keyboard and mouse events.
    pub input: Receiver<InputEvent>,
    /// Layout change notifications.
    pub layout: Option<Receiver<LayoutId>>,
    /// Focus change notifications.
    pub focus: Option<Receiver<FocusEvent>>,
    /// Clipboard change notifications («Следить за буфером обмена»).
    pub clipboard: Option<Receiver<()>>,
}

/// A running engine thread.
#[derive(Debug)]
pub struct EngineHandle {
    commands: Sender<Command>,
    events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}

impl EngineHandle {
    /// Starts the engine thread with a processor and its input channels.
    pub fn spawn(processor: Processor, inputs: Inputs) -> std::io::Result<Self> {
        let (commands, command_rx) = unbounded();
        let (event_tx, events) = unbounded();
        let thread = std::thread::Builder::new()
            .name("okbs-engine".to_string())
            .spawn(move || run(processor, &command_rx, &inputs, &event_tx))?;
        Ok(Self {
            commands,
            events,
            thread: Some(thread),
        })
    }

    /// Sends a command; returns `false` if the engine has already stopped.
    pub fn send(&self, command: Command) -> bool {
        self.commands.send(command).is_ok()
    }

    /// A sender for commands, e.g. for the tray thread.
    pub fn command_sender(&self) -> Sender<Command> {
        self.commands.clone()
    }

    /// Engine notifications.
    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }

    /// Stops the engine and waits for its thread.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("engine thread panicked");
        }
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_command(processor: &mut Processor, command: Command) -> Vec<Event> {
    match command {
        Command::SetAutoswitch(on) => {
            if processor.set_autoswitch(on) {
                tracing::info!(on, "autoswitch changed");
                vec![Event::AutoswitchChanged(on)]
            } else {
                Vec::new()
            }
        }
        Command::ToggleAutoswitch => {
            let on = !processor.config().general.autoswitch;
            handle_command(processor, Command::SetAutoswitch(on))
        }
        Command::SetSounds(on) => {
            if processor.set_sounds(on) {
                vec![Event::SoundsChanged(on)]
            } else {
                Vec::new()
            }
        }
        Command::ApplyConfig(config) => {
            let mut events = Vec::new();
            if config.general.autoswitch != processor.config().general.autoswitch {
                events.push(Event::AutoswitchChanged(config.general.autoswitch));
            }
            if config.sounds.enabled != processor.config().sounds.enabled {
                events.push(Event::SoundsChanged(config.sounds.enabled));
            }
            processor.apply_config(*config);
            events.extend(processor.autoreplace_feedback());
            tracing::info!("configuration applied");
            events.push(Event::ConfigApplied);
            events
        }
        Command::ClipboardOp(op) => processor.clipboard_op(op),
        Command::SpellcheckClipboard => processor.spellcheck_clipboard(),
        Command::CorrectClipboard {
            original,
            corrected,
        } => processor.correct_clipboard(&original, &corrected),
        Command::InsertAutoreplace { item, target } => processor.insert_autoreplace(item, target),
        Command::InsertText { text, target } => processor.insert_text(&text, target),
        Command::AutoreplaceList { toggle } => processor.autoreplace_list(toggle),
        Command::CaptureHotkey => {
            processor.set_capture(true);
            processor.autoreplace_feedback()
        }
        Command::CancelCapture => {
            processor.set_capture(false);
            processor.autoreplace_feedback()
        }
        Command::Shutdown => Vec::new(),
    }
}

fn run(
    mut processor: Processor,
    commands: &Receiver<Command>,
    inputs: &Inputs,
    events: &Sender<Event>,
) {
    tracing::debug!("engine started");
    let _ = events.send(Event::Started);
    let mut layout_rx = inputs.layout.clone().unwrap_or_else(never);
    let mut focus_rx = inputs.focus.clone().unwrap_or_else(never);
    let mut clipboard_rx = inputs.clipboard.clone().unwrap_or_else(never);
    let emit = |list: Vec<Event>| {
        for event in list {
            let _ = events.send(event);
        }
    };
    loop {
        select! {
            default(std::time::Duration::from_millis(200)) => emit(processor.autoreplace_feedback()),
            recv(commands) -> command => match command {
                Ok(Command::Shutdown) | Err(_) => break,
                Ok(command) => emit(handle_command(&mut processor, command)),
            },
            recv(inputs.input) -> event => match event {
                Ok(event) => emit(processor.handle_input(event)),
                Err(_) => {
                    tracing::warn!("input source closed");
                    break;
                }
            },
            recv(layout_rx) -> id => {
                match id {
                    Ok(id) => emit(processor.handle_layout_change(id)),
                    Err(_) => layout_rx = never(),
                }
            },
            recv(focus_rx) -> focus => {
                match focus {
                    Ok(focus) => {
                        processor.handle_focus(&focus);
                        emit(processor.autoreplace_feedback());
                    }
                    Err(_) => focus_rx = never(),
                }
            },
            recv(clipboard_rx) -> change => {
                match change {
                    Ok(()) => emit(processor.handle_clipboard_change()),
                    Err(_) => clipboard_rx = never(),
                }
            },
        }
    }
    tracing::debug!("engine stopped");
    let _ = events.send(Event::Stopped);
}

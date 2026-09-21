//! The application on Linux. Keyboard capture arrives in stage 3; until then
//! the engine gets no input, but the tray and the settings window work.

use crate::controller::Controller;
use crate::settings::Settings;
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, unbounded};
use okbs_core::{KeyMap, Lang};
use okbs_engine::{Backends, EngineHandle, Inputs, Processor};
use okbs_platform::{Injector, KeyStroke, LayoutId, LayoutInfo, LayoutManager, PlatformError};
use okbs_ui::settings::Section;
use std::time::Duration;

struct NoInjector;
impl Injector for NoInjector {
    fn send(&mut self, _strokes: &[KeyStroke]) -> okbs_platform::Result<()> {
        Err(PlatformError::Unsupported(
            "key injection on Linux (stage 3)",
        ))
    }
}

struct NoLayouts;
impl LayoutManager for NoLayouts {
    fn layouts(&self) -> okbs_platform::Result<Vec<LayoutInfo>> {
        Ok(Vec::new())
    }
    fn current(&self) -> okbs_platform::Result<LayoutId> {
        Err(PlatformError::Unsupported("layouts on Linux (stage 3)"))
    }
    fn set(&mut self, _layout: LayoutId) -> okbs_platform::Result<()> {
        Err(PlatformError::Unsupported("layouts on Linux (stage 3)"))
    }
    fn keymap(&self, layout: LayoutId) -> okbs_platform::Result<KeyMap> {
        let _ = layout;
        Ok(okbs_core::layouts::builtin_keymap(Lang::En).clone())
    }
}

/// Runs until the user chooses «Выйти» or `stop` fires.
pub fn run(
    settings: Settings,
    no_tray: bool,
    open_settings: bool,
    stop: &Receiver<()>,
) -> Result<()> {
    tracing::warn!("keyboard capture is not implemented on Linux yet (stage 3)");
    let (_input_tx, input_rx) = unbounded();
    let processor = Processor::new(
        settings.config.clone(),
        Backends {
            injector: Box::new(NoInjector),
            layouts: Box::new(NoLayouts),
            clipboard: None,
            sound: None,
            focus: None,
        },
    );
    let engine = EngineHandle::spawn(
        processor,
        Inputs {
            input: input_rx,
            layout: None,
            focus: None,
            clipboard: None,
        },
    )
    .context("cannot start the engine")?;
    let mut controller = Controller::new(
        settings,
        engine,
        crate::controller::PlatformHooks::default(),
        None,
        !no_tray,
    );
    if open_settings {
        controller.open_settings(Section::General);
    }
    loop {
        match stop.recv_timeout(Duration::from_millis(40)) {
            Ok(()) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        if !controller.step() {
            break;
        }
    }
    controller.shutdown();
    Ok(())
}

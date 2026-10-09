//! The common engine with native Linux input, layout and desktop services.
use crate::{
    controller::{Controller, PlatformHooks},
    paths::AppPaths,
    settings::Settings,
};
use anyhow::Result;
use crossbeam_channel::{Receiver, unbounded};
use okbs_core::config::Config;
use okbs_engine::{Backends, EngineHandle, Inputs, Processor};
use okbs_platform::{
    Autostart, Clipboard, FocusInfo, InputEvent, KeyboardSource, LayoutInfo, LayoutManager,
    StopGuard,
};
use okbs_platform_linux::services::{
    LinuxAutostart, LinuxClipboard, LinuxFileDialogs, LinuxSound, LinuxSystemSettings,
};
use okbs_platform_linux::{
    desktop::LinuxDesktop,
    input::{LinuxInjector, LinuxSource},
};
use okbs_ui::settings::Section;
use std::sync::Arc;
use std::time::Duration;

struct EngineRuntime {
    engine: EngineHandle,
    layout: Option<LayoutInfo>,
    layout_watch: Box<dyn StopGuard>,
    focus_watch: Box<dyn StopGuard>,
    clipboard_watch: Option<Box<dyn StopGuard>>,
}
fn prepare_engine(
    config: &Config,
    mut desktop: LinuxDesktop,
    source: &LinuxSource,
    injector: LinuxInjector,
    input: Receiver<InputEvent>,
) -> Result<EngineRuntime> {
    let (layout_tx, layout_rx) = unbounded();
    let (focus_tx, focus_rx) = unbounded();
    let (clipboard_tx, clipboard_rx) = unbounded();
    let layout_watch = LayoutManager::subscribe(&mut desktop, layout_tx)?;
    let focus_watch = FocusInfo::subscribe(&mut desktop, focus_tx)?;
    let mut clipboard = LinuxClipboard::new()?;
    let clipboard_watch = clipboard.subscribe(clipboard_tx).ok();
    let layout = desktop
        .layouts()?
        .into_iter()
        .find(|layout| Some(layout.id) == desktop.current().ok());
    let mut processor = Processor::new(
        config.clone(),
        Backends {
            injector: Box::new(injector),
            layouts: Box::new(desktop.clone()),
            clipboard: Some(Box::new(LinuxClipboard::new()?)),
            sound: Some(Box::new(LinuxSound)),
            focus: Some(Box::new(desktop)),
        },
    );
    if let Some(caps) = source.caps_lock_on() {
        processor.set_caps_lock(caps);
    }
    processor.set_swallows_capslock(true);
    processor.set_input_gate(source.filter.gate.clone());
    let engine = EngineHandle::spawn(
        processor,
        Inputs {
            input,
            layout: Some(layout_rx),
            focus: Some(focus_rx),
            clipboard: clipboard_watch.is_some().then_some(clipboard_rx),
        },
    )?;
    Ok(EngineRuntime {
        engine,
        layout,
        layout_watch,
        focus_watch,
        clipboard_watch,
    })
}

pub fn run(
    mut settings: Settings,
    paths: &AppPaths,
    no_tray: bool,
    open_settings: bool,
    first_run: bool,
    stop: &Receiver<()>,
) -> Result<()> {
    let first_start = crate::linux_startup::FirstRun::begin(&paths.state_dir, first_run);
    let first_run = first_start.pending;
    let (input_tx, input_rx) = unbounded();
    let Some(ready) = crate::linux_startup::run(
        &mut settings,
        first_run,
        input_tx,
        stop,
        |config, desktop, source, injector| {
            prepare_engine(config, desktop.clone(), source, injector, input_rx.clone())
        },
    )?
    else {
        return Ok(());
    };
    let crate::linux_startup::Ready {
        desktop,
        source,
        runtime,
        capture,
        bridge: _bridge,
        window: settings_window,
    } = ready;
    let EngineRuntime {
        engine,
        layout,
        layout_watch,
        focus_watch,
        clipboard_watch,
    } = runtime;
    let config = settings.config.clone();
    let panels = okbs_platform_linux::panels::Panels::new(desktop.clone())?;
    let autostart = LinuxAutostart::new()?;
    autostart.set_enabled(config.general.autostart, &std::env::current_exe()?, false)?;
    let filter = source.filter.clone();
    let list_desktop = desktop.clone();
    let cursor_desktop = desktop.clone();
    let caret_desktop = desktop.clone();
    let popup_desktop = desktop.clone();
    let indicator_panels = panels.clone();
    let picker_panels = panels;
    let picker_desktop = desktop.clone();
    let picker_gate = filter.gate.clone();
    let picker_settings = settings.config.autoreplace.clone();
    let history_desktop = desktop.clone();
    let picker_theme = settings.config.general.theme;
    let picker_labels = crate::controller::autoreplace_labels(
        settings
            .config
            .general
            .ui_language
            .resolve(crate::locale::system_ui_language()),
    );
    let configure_desktop = desktop.clone();
    let configure_panels = picker_panels.clone();
    let mut controller = Controller::new(
        settings,
        engine,
        PlatformHooks {
            settings_window,
            popup_focus: Some(Box::new(move |title| {
                let Some(window) = history_desktop.placed_window(title)? else {
                    return Ok(false);
                };
                history_desktop.command("ActivateWindow", window.window)?;
                Ok(true)
            })),
            popup_placement: Some(Box::new(move |title, position| {
                popup_desktop.place_owned_window(title, position)
            })),
            cursor_position: Some(Box::new(move || {
                let s = cursor_desktop.cached();
                [s.cursor[0] as f32, s.cursor[1] as f32]
            })),
            spelling_position: Some(Box::new(move |target| {
                let point = caret_desktop.popup_position(target);
                if let Some(point) = point {
                    for language in okbs_core::Lang::ALL {
                        let _ = caret_desktop.place_passive_window(
                            okbs_ui::tr(okbs_ui::Text::SpellcheckWordTitle, language),
                            point,
                        );
                    }
                }
                point
            })),
            indicator: Some(Box::new(move |state, labels| {
                Ok(Box::new(okbs_platform_linux::panels::LinuxIndicator::new(
                    indicator_panels,
                    state,
                    labels,
                )?)
                    as Box<dyn okbs_platform::FloatingIndicator>)
            })),
            autoreplace_ui: Some(Box::new(move |window| {
                Ok(Box::new(crate::linux_ui::LinuxAutoreplaceUi::new(
                    picker_panels,
                    picker_desktop,
                    window.autoreplace_list(),
                    picker_gate,
                    picker_settings,
                    picker_labels,
                    picker_theme,
                )) as Box<dyn okbs_platform::AutoreplaceUi>)
            })),
            list_layouts: Some(Box::new(move || list_desktop.layouts().unwrap_or_default())),
            sound: Some(Box::new(LinuxSound)),
            autostart: Some(Box::new(autostart)),
            window_control: Some(Box::new(desktop)),
            history_file: Some(paths.state_dir.join(crate::paths::HISTORY_FILE)),
            on_apply: Some(Box::new(move |config| {
                if let Some(prepared) = configure_desktop.prepare_layout(config)? {
                    filter.gate.fail_open();
                    configure_panels.suspend();
                    configure_desktop.apply_layout(prepared)?;
                }
                filter.configure(config);
                Ok(())
            })),
            file_dialogs: Some(Arc::new(LinuxFileDialogs)),
            system_settings: Some(Box::new(LinuxSystemSettings)),
            ..PlatformHooks::default()
        },
        layout,
        !no_tray,
    );
    if open_settings {
        controller.open_settings(Section::General);
    }
    if first_run {
        controller.suggest_exclusions();
    }
    first_start.finish();
    loop {
        match stop.recv_timeout(Duration::from_millis(40)) {
            Ok(()) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        if !controller.step() {
            break;
        }
        if !source.is_alive() {
            tracing::error!("Linux input capture stopped; physical keyboards have been released");
            break;
        }
    }
    controller.shutdown();
    drop(capture);
    drop((layout_watch, focus_watch, clipboard_watch));
    Ok(())
}

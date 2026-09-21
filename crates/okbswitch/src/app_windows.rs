//! The application on Windows: hooks, engine thread, tray and settings window.

use crate::controller::Controller;
use crate::settings::Settings;
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, unbounded};
use okbs_core::config::{AutoReplace, Theme};
use okbs_engine::{Backends, EngineHandle, Inputs, Processor};
use okbs_platform::{
    AutoreplaceInsertion, AutoreplaceLabels, AutoreplaceUi, Clipboard, FocusInfo, InputTarget,
    KeyboardSource, LayoutManager, Result as PlatformResult,
};
use okbs_platform_windows::{
    HookFilter, HookSource, SendInputInjector, WinAutoreplaceUi, WinAutostart, WinClipboard,
    WinElevation, WinFocus, WinIndicator, WinLayouts, WinSound, WinWindowControl,
};
use okbs_ui::autoreplace_list::{
    AutoreplaceListConfig, AutoreplaceListLabels, AutoreplaceListWindow,
};
use okbs_ui::settings::Section;
use std::time::Duration;

/// Restarts the program with administrator rights when «Запускать с правами
/// Администратора» is on and this process does not have them. A declined UAC
/// prompt is not fatal: the program keeps running without the rights, and the
/// settings window explains what that means.
fn elevate_if_requested(
    config: &okbs_core::config::Config,
    paths: &crate::paths::AppPaths,
) -> bool {
    use okbs_platform::Elevation;
    if !config.general.run_elevated || WinElevation.is_elevated() {
        return false;
    }
    let arguments = vec![
        "--restarting".to_string(),
        "--config".to_string(),
        paths.config_file.display().to_string(),
    ];
    match std::env::current_exe()
        .map_err(okbs_platform::PlatformError::from)
        .and_then(|exe| WinElevation.restart_elevated(&exe, &arguments))
    {
        Ok(()) => {
            tracing::info!("starting again with administrator rights");
            true
        }
        Err(err) => {
            tracing::warn!("running without administrator rights: {err}");
            false
        }
    }
}

/// The hint stays a native non-activating label, while the permanent list is
/// rendered by the shared egui UI so its styling exactly follows Settings.
struct ThemedAutoreplaceUi {
    hint: WinAutoreplaceUi,
    list: AutoreplaceListWindow,
    settings: AutoReplace,
    labels: AutoreplaceLabels,
    theme: Theme,
    target: Option<InputTarget>,
    persistent: bool,
}

impl std::fmt::Debug for ThemedAutoreplaceUi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemedAutoreplaceUi")
            .finish_non_exhaustive()
    }
}

impl ThemedAutoreplaceUi {
    fn new(
        settings: &AutoReplace,
        labels: AutoreplaceLabels,
        theme: Theme,
        gate: std::sync::Arc<okbs_platform::autoreplace_gate::AutoReplaceGate>,
        list: AutoreplaceListWindow,
    ) -> PlatformResult<Self> {
        Ok(Self {
            hint: WinAutoreplaceUi::new(settings, labels.clone(), gate)?,
            list,
            settings: settings.clone(),
            labels,
            theme,
            target: None,
            persistent: false,
        })
    }

    fn list_config(&self) -> AutoreplaceListConfig {
        AutoreplaceListConfig {
            settings: self.settings.clone(),
            labels: AutoreplaceListLabels {
                title: self.labels.title.clone(),
                insert: self.labels.insert.clone(),
                close: self.labels.close.clone(),
                empty: self.labels.empty.clone(),
                disabled: self.labels.disabled.clone(),
                menu_help: self.labels.menu_help.clone(),
                list_help: self.labels.list_help.clone(),
            },
            theme: self.theme,
        }
    }
}

impl AutoreplaceUi for ThemedAutoreplaceUi {
    fn configure(
        &mut self,
        settings: &AutoReplace,
        labels: AutoreplaceLabels,
        theme: Theme,
    ) -> PlatformResult<()> {
        self.hint.configure(settings, labels.clone(), theme)?;
        self.settings = settings.clone();
        self.labels = labels;
        self.theme = theme;
        self.list.configure(self.list_config());
        Ok(())
    }

    fn hint(&mut self, index: Option<usize>) -> PlatformResult<()> {
        self.hint.hint(index)
    }

    fn show_list(&mut self, toggle: bool, target: Option<InputTarget>) -> PlatformResult<()> {
        self.hint.hint(None)?;
        if toggle && self.list.is_visible() {
            self.list.hide();
            return Ok(());
        }
        self.target = target;
        self.persistent = toggle;
        self.list.show(
            self.list_config(),
            toggle,
            okbs_platform_windows::focus::cursor_position(),
        );
        Ok(())
    }

    fn poll(&mut self) -> Option<AutoreplaceInsertion> {
        let index = self.list.poll()?;
        let item = self.settings.items.get(index)?.clone();
        let target = if self.persistent {
            okbs_platform_windows::focus::input_target().or(self.target)
        } else {
            self.target
        };
        Some(AutoreplaceInsertion { item, target })
    }
}

/// Runs until the user chooses «Выйти» or `stop` fires.
pub fn run(
    settings: Settings,
    paths: &crate::paths::AppPaths,
    no_tray: bool,
    open_settings: bool,
    stop: &Receiver<()>,
) -> Result<()> {
    if elevate_if_requested(&settings.config, paths) {
        return Ok(());
    }
    let config = settings.config.clone();
    let (input_tx, input_rx) = unbounded();
    let filter = std::sync::Arc::new(HookFilter::default());
    filter.configure(&config);
    let mut source = HookSource {
        filter: filter.clone(),
    };
    let caps = source.caps_lock_on();
    let _capture = source
        .start(input_tx)
        .context("cannot install the keyboard hook")?;

    let mut layouts = WinLayouts;
    let (layout_tx, layout_rx) = unbounded();
    let layout_watch = layouts.subscribe(layout_tx).ok();
    let mut focus = WinFocus;
    let (focus_tx, focus_rx) = unbounded();
    let focus_watch = focus.subscribe(focus_tx).ok();
    // «Следить за буфером обмена»: the notification carries no text, and the
    // engine decides whether the new contents are worth remembering.
    let mut clipboard = WinClipboard;
    let (clipboard_tx, clipboard_rx) = unbounded();
    let clipboard_watch = match clipboard.subscribe(clipboard_tx) {
        Ok(watch) => Some(watch),
        Err(err) => {
            tracing::warn!("clipboard history unavailable: {err}");
            None
        }
    };

    std::thread::spawn(okbs_core::data::warm_up);
    let backends = Backends {
        injector: Box::new(SendInputInjector {
            key_delay: Duration::from_millis(u64::from(config.switching.inject_key_delay_ms)),
        }),
        layouts: Box::new(layouts),
        clipboard: Some(Box::new(WinClipboard)),
        sound: Some(Box::new(WinSound)),
        focus: Some(Box::new(WinFocus)),
    };
    let mut processor = Processor::new(config, backends);
    if let Some(on) = caps {
        processor.set_caps_lock(on);
    }
    processor.set_swallows_capslock(true);
    processor.set_input_gate(filter.autoreplace.clone());
    let engine = EngineHandle::spawn(
        processor,
        Inputs {
            input: input_rx,
            layout: Some(layout_rx),
            focus: Some(focus_rx),
            clipboard: clipboard_watch.is_some().then_some(clipboard_rx),
        },
    )
    .context("cannot start the engine")?;

    let layout = okbs_platform_windows::layouts::foreground_layout()
        .map(okbs_platform_windows::layouts::describe);
    let list_settings = settings.config.autoreplace.clone();
    let list_labels = crate::controller::autoreplace_labels(
        settings
            .config
            .general
            .ui_language
            .resolve(crate::locale::system_ui_language()),
    );
    let theme = settings.config.general.theme;
    let gate = filter.autoreplace.clone();
    let autoreplace_ui: Option<crate::controller::AutoreplaceUiFactory> =
        Some(Box::new(move |window| {
            ThemedAutoreplaceUi::new(
                &list_settings,
                list_labels,
                theme,
                gate,
                window.autoreplace_list(),
            )
            .map(|popup| Box::new(popup) as Box<dyn AutoreplaceUi>)
        }));
    let indicator: Option<crate::controller::IndicatorFactory> = Some(Box::new(|state, labels| {
        WinIndicator::new(state, labels)
            .map(|indicator| Box::new(indicator) as Box<dyn okbs_platform::FloatingIndicator>)
    }));
    let mut controller = Controller::new(
        settings,
        engine,
        crate::controller::PlatformHooks {
            autoreplace_ui,
            list_layouts: Some(Box::new(okbs_platform_windows::layouts::installed_layouts)),
            sound: Some(Box::new(WinSound)),
            autostart: Some(Box::new(WinAutostart)),
            elevation: Some(Box::new(WinElevation)),
            window_control: Some(Box::new(WinWindowControl)),
            indicator,
            history_file: Some(paths.state_dir.join(crate::paths::HISTORY_FILE)),
            on_apply: Some(Box::new(move |config| filter.configure(config))),
        },
        layout,
        !no_tray,
    );
    if open_settings {
        controller.open_settings(Section::General);
    }
    tracing::info!("running");
    while okbs_platform_windows::pump_messages(Duration::from_millis(40))
        && stop.try_recv().is_err()
    {
        if !controller.step() {
            break;
        }
    }
    tracing::info!("stopping");
    controller.shutdown();
    drop(layout_watch);
    drop(focus_watch);
    drop(clipboard_watch);
    Ok(())
}

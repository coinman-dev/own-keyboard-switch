//! Linux setup runs before keyboard capture and reuses the normal GUI thread.
use crate::settings::Settings;
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, unbounded};
use okbs_core::config::{Config, LayoutBackend};
use okbs_platform::{InputEvent, KeyboardSource, LayoutManager, StopGuard, SystemSettings};
use okbs_platform_linux::{
    access::{self, DeviceAccess, InputScan},
    desktop::{BridgeConnection, LinuxDesktop, serve_kde_bridge},
    input::{LinuxInjector, LinuxSource, VIRTUAL_NAME},
    integration::{self, Activation},
    permissions::{self, PendingRequest},
    seat::Seat,
    services::LinuxSystemSettings,
    session::{Desktop, SessionInfo},
};
use okbs_ui::{
    Text,
    linux_setup::{Action, DesktopAccess, InputAccess, SessionAccess, Status},
    settings::{SettingsEvent, SettingsInput},
    tr,
    window::SettingsWindow,
};
use std::time::{Duration, Instant};

pub type WindowBundle = (SettingsWindow, Receiver<SettingsEvent>);
pub struct FirstRun {
    marker: std::path::PathBuf,
    pub pending: bool,
}
impl FirstRun {
    pub fn begin(directory: &std::path::Path, new_profile: bool) -> Self {
        let marker = directory.join("linux-first-start.pending");
        if new_profile && let Err(err) = std::fs::write(&marker, b"pending\n") {
            tracing::warn!("cannot remember pending Linux setup: {err}");
        }
        Self {
            pending: new_profile || marker.is_file(),
            marker,
        }
    }
    pub fn finish(self) {
        if self.pending
            && let Err(err) = std::fs::remove_file(&self.marker)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("cannot finish the Linux first-start record: {err}");
        }
    }
}
pub struct Ready<T> {
    pub desktop: LinuxDesktop,
    pub source: LinuxSource,
    pub runtime: T,
    pub capture: Box<dyn StopGuard>,
    pub bridge: Option<BridgeConnection>,
    pub window: Option<WindowBundle>,
}

struct Probe {
    seat: Seat,
    desktop: Option<LinuxDesktop>,
    bridge: Option<BridgeConnection>,
    session: SessionInfo,
}
impl Probe {
    fn new() -> Self {
        Self {
            seat: Seat::connect(),
            desktop: None,
            bridge: None,
            session: SessionInfo::detect(),
        }
    }
    fn inspect(&mut self, config: &Config) -> Status {
        if self.bridge.is_none() {
            self.bridge = serve_kde_bridge()
                .map_err(|err| tracing::debug!("desktop bridge unavailable: {err}"))
                .ok();
        }
        let scan = access::scan_input_devices();
        let input = input_access(access::uinput_access(), &scan, &config.linux.devices);
        let seat = self.seat.state();
        let session = if permissions::is_root() {
            SessionAccess::Root
        } else if seat.active && !seat.locked {
            SessionAccess::Ready
        } else {
            SessionAccess::Inactive
        };
        let desktop = if !backend_available(config.linux.layout_backend, &self.session) {
            DesktopAccess::Unsupported
        } else {
            if self
                .desktop
                .as_ref()
                .is_some_and(|desktop| desktop.refresh().is_err())
            {
                self.desktop = None;
            }
            if self.desktop.is_none() {
                self.desktop = LinuxDesktop::connect_with(config.linux.layout_backend)
                    .map_err(|err| tracing::debug!("desktop setup check: {err}"))
                    .ok();
            }
            match &self.desktop {
                Some(desktop) if desktop.integration_ready() => match desktop.layouts() {
                    Ok(layouts)
                        if config.general.language_pair.iter().all(|language| {
                            layouts.iter().any(|layout| layout.lang == Some(*language))
                        }) =>
                    {
                        DesktopAccess::Ready
                    }
                    Ok(_) => DesktopAccess::MissingLayouts,
                    Err(_) => DesktopAccess::Unavailable,
                },
                _ if matches!(self.session.desktop, Desktop::Gnome | Desktop::Kde) => {
                    DesktopAccess::NeedsIntegration
                }
                _ => DesktopAccess::Unavailable,
            }
        };
        Status {
            input,
            desktop,
            session,
            busy: false,
            error: None,
            session_restart: false,
            selected_devices: !config.linux.devices.is_empty(),
        }
    }
    fn start<T>(
        &self,
        config: &Config,
        sink: Sender<InputEvent>,
        prepare: &mut impl FnMut(&Config, &LinuxDesktop, &LinuxSource, LinuxInjector) -> Result<T>,
    ) -> Result<(LinuxSource, T, Box<dyn StopGuard>)> {
        let desktop = self.desktop.as_ref().context("desktop is not connected")?;
        let (mut source, injector) = LinuxSource::new(config, desktop.clone())?;
        let runtime = prepare(config, desktop, &source, injector)?;
        let capture = source.start(sink)?;
        Ok((source, runtime, capture))
    }
}

fn backend_available(backend: LayoutBackend, session: &SessionInfo) -> bool {
    okbs_platform_linux::session::validate_layout_backend(backend, session).is_ok()
}
fn input_access(uinput: DeviceAccess, scan: &InputScan, selected: &[String]) -> InputAccess {
    match uinput {
        DeviceAccess::Denied | DeviceAccess::Missing => return InputAccess::NeedsSetup,
        DeviceAccess::Failed => return InputAccess::Unavailable,
        DeviceAccess::Granted => {}
    }
    let selected_paths: Vec<_> = selected
        .iter()
        .filter_map(|path| std::fs::canonicalize(path).ok())
        .collect();
    if scan.keyboards.iter().any(|keyboard| {
        keyboard.name != VIRTUAL_NAME
            && (selected.is_empty()
                || std::fs::canonicalize(&keyboard.path)
                    .is_ok_and(|path| selected_paths.contains(&path)))
    }) {
        return InputAccess::Ready;
    }
    if !scan.input_dir_exists
        || scan.denied > 0 && (selected.is_empty() || !selected_paths.is_empty())
    {
        InputAccess::NeedsSetup
    } else {
        InputAccess::NoKeyboard
    }
}

enum Job {
    Permission(PendingRequest),
    Integration(Receiver<okbs_platform::Result<Activation>>),
}
impl Job {
    fn poll(&mut self) -> Option<okbs_platform::Result<Activation>> {
        match self {
            Self::Permission(request) => match request.poll() {
                Ok(None) => None,
                Ok(Some(())) => Some(Ok(Activation::Enabled)),
                Err(err) => Some(Err(err)),
            },
            Self::Integration(result) => match result.try_recv() {
                Ok(result) => Some(result),
                Err(crossbeam_channel::TryRecvError::Empty) => None,
                Err(_) => Some(Err(okbs_platform::PlatformError::Other(
                    "desktop setup worker stopped".into(),
                ))),
            },
        }
    }
}

pub fn run<T>(
    settings: &mut Settings,
    first_run: bool,
    sink: Sender<InputEvent>,
    stop: &Receiver<()>,
    mut prepare: impl FnMut(&Config, &LinuxDesktop, &LinuxSource, LinuxInjector) -> Result<T>,
) -> Result<Option<Ready<T>>> {
    let mut probe = Probe::new();
    let mut status = probe.inspect(&settings.config);
    let mut window: Option<WindowBundle> = None;
    let mut sent_status = None;
    let mut sent_config = settings.config.clone();
    let mut job: Option<Job> = None;
    let mut last_check = Instant::now();
    let mut auto_authorize = settings.config.general.run_elevated;
    let mut restart_hint = false;
    let mut failed_capture = false;
    let mut requested = (!first_run && status.ready()).then_some(Action::Continue);
    loop {
        if !matches!(stop.try_recv(), Err(crossbeam_channel::TryRecvError::Empty)) {
            return Ok(None);
        }
        if let Some((ui, events)) = &window {
            if !ui.is_alive() {
                anyhow::bail!("the setup window could not be opened");
            }
            for event in events.try_iter() {
                match event {
                    SettingsEvent::Closed => return Ok(None),
                    SettingsEvent::LinuxSetup(action) => requested = Some(action),
                    SettingsEvent::Apply(config) => {
                        settings.update(|current| {
                            current.general.run_elevated = config.general.run_elevated
                        });
                        auto_authorize = settings.config.general.run_elevated;
                    }
                    // The modal blocks ordinary settings edits until startup.
                    _ => {}
                }
            }
        }
        if auto_authorize
            && window.as_ref().is_some_and(|(ui, _)| ui.is_open())
            && status.allows(Action::Authorize)
            && requested.is_none()
        {
            requested = Some(Action::Authorize);
            auto_authorize = false;
        }
        if let Some(action) = requested.take()
            && status.allows(action)
        {
            match action {
                Action::Continue => {
                    let checked = probe.inspect(&settings.config);
                    if checked.ready() {
                        match probe.start(&settings.config, sink.clone(), &mut prepare) {
                            Ok((source, runtime, capture)) => {
                                if let Some((ui, _)) = &window {
                                    ui.send(SettingsInput::LinuxSetupConfig(Box::new(
                                        settings.config.clone(),
                                    )));
                                    ui.send(SettingsInput::LinuxSetupFinished);
                                }
                                return Ok(Some(Ready {
                                    desktop: probe
                                        .desktop
                                        .take()
                                        .context("desktop connection disappeared during startup")?,
                                    source,
                                    runtime,
                                    capture,
                                    bridge: probe.bridge.take(),
                                    window,
                                }));
                            }
                            Err(err) => {
                                tracing::warn!("input startup failed: {err:#}");
                                failed_capture = true;
                                status.input = InputAccess::Unavailable;
                            }
                        }
                    } else {
                        status = checked;
                    }
                }
                Action::Quit => return Ok(None),
                Action::Authorize => {
                    auto_authorize = false;
                    match PendingRequest::start() {
                        Ok(request) => {
                            job = Some(Job::Permission(request));
                            status.busy = true;
                            status.error = None;
                        }
                        Err(err) => {
                            tracing::warn!("input authorization failed: {err}");
                            status.error = Some(message(settings, Text::LinuxAuthFailed));
                        }
                    }
                }
                Action::InstallIntegration => {
                    let (result, receiver) = unbounded();
                    let desktop = probe.session.desktop.clone();
                    std::thread::Builder::new()
                        .name("okbs-desktop-setup".into())
                        .spawn(move || {
                            let _ = result.send(integration::install_current(&desktop));
                        })?;
                    job = Some(Job::Integration(receiver));
                    status.busy = true;
                    status.error = None;
                }
                Action::UseAutoBackend => {
                    settings.update(|config| config.linux.layout_backend = LayoutBackend::Auto);
                    probe.desktop = None;
                }
                Action::UseAllKeyboards => {
                    settings.update(|config| config.linux.devices.clear());
                }
                Action::KeyboardSettings => {
                    if let Err(err) = LinuxSystemSettings.open_keyboard_settings() {
                        tracing::warn!("keyboard settings unavailable: {err}");
                        status.error = Some(message(settings, Text::LinuxDesktopUnavailable));
                    }
                }
                Action::Recheck => {
                    failed_capture = false;
                    status.error = None;
                }
            }
            last_check = Instant::now() - Duration::from_secs(2);
        }
        if let Some(completion) = job.as_mut().and_then(Job::poll) {
            let integration_job = matches!(job, Some(Job::Integration(_)));
            job = None;
            status.busy = false;
            match completion {
                Ok(activation) => {
                    restart_hint = activation == Activation::SessionRestart;
                    status.error = None;
                    if integration_job && probe.session.desktop == Desktop::Gnome {
                        settings.update(|config| config.linux.gnome_extension_installed = true);
                    }
                }
                Err(err) => {
                    tracing::warn!("Linux setup failed: {err}");
                    status.error = Some(message(
                        settings,
                        if integration_job {
                            Text::LinuxIntegrationFailed
                        } else {
                            Text::LinuxAuthFailed
                        },
                    ));
                }
            }
            last_check = Instant::now() - Duration::from_secs(2);
        }
        if job.is_none() && last_check.elapsed() >= Duration::from_secs(2) {
            let error = status.error.take();
            status = probe.inspect(&settings.config);
            status.error = error;
            status.session_restart = restart_hint && status.desktop != DesktopAccess::Ready;
            if failed_capture {
                status.input = InputAccess::Unavailable;
            }
            last_check = Instant::now();
        }
        if window.is_none() {
            let (events, receiver) = unbounded();
            let ui = SettingsWindow::spawn(events, crate::locale::system_ui_language())?;
            ui.show_linux_setup(&settings.config, status.clone());
            sent_status = Some(status.clone());
            window = Some((ui, receiver));
        } else if let Some((ui, _)) = &window
            && sent_status.as_ref() != Some(&status)
        {
            ui.send(SettingsInput::LinuxSetup(status.clone()));
            sent_status = Some(status.clone());
        }
        if sent_config != settings.config
            && let Some((ui, _)) = &window
        {
            ui.send(SettingsInput::LinuxSetupConfig(Box::new(
                settings.config.clone(),
            )));
            sent_config = settings.config.clone();
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}
fn message(settings: &Settings, text: Text) -> String {
    tr(
        text,
        settings
            .config
            .general
            .ui_language
            .resolve(crate::locale::system_ui_language()),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use okbs_platform_linux::access::KeyboardDevice;
    #[test]
    fn cancelled_first_setup_survives_a_session_restart() {
        let scratch =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp/linux-startup");
        std::fs::create_dir_all(&scratch).unwrap();
        let directory = tempfile::TempDir::new_in(scratch).unwrap();
        let first = FirstRun::begin(directory.path(), true);
        assert!(first.pending);
        drop(first);
        let resumed = FirstRun::begin(directory.path(), false);
        assert!(resumed.pending);
        resumed.finish();
        assert!(!FirstRun::begin(directory.path(), false).pending);
    }
    #[test]
    fn input_check_does_not_ask_for_unrelated_devices_or_use_its_own_keyboard() {
        let keyboard = KeyboardDevice {
            path: "/dev/fixture".into(),
            name: "Fixture keyboard".into(),
        };
        let scan = InputScan {
            input_dir_exists: true,
            event_nodes: 2,
            denied: 1,
            keyboards: vec![keyboard],
        };
        assert_eq!(
            input_access(DeviceAccess::Granted, &scan, &[]),
            InputAccess::Ready
        );
        assert_eq!(
            input_access(DeviceAccess::Denied, &scan, &[]),
            InputAccess::NeedsSetup
        );
        assert_eq!(
            input_access(DeviceAccess::Missing, &scan, &[]),
            InputAccess::NeedsSetup
        );
        assert_eq!(
            input_access(DeviceAccess::Failed, &scan, &[]),
            InputAccess::Unavailable
        );
        assert_eq!(
            input_access(
                DeviceAccess::Granted,
                &scan,
                &["/nonexistent/okbs-fixture".into()]
            ),
            InputAccess::NoKeyboard
        );
        let own = InputScan {
            denied: 0,
            keyboards: vec![KeyboardDevice {
                path: "/dev/fixture".into(),
                name: VIRTUAL_NAME.into(),
            }],
            ..scan
        };
        assert_eq!(
            input_access(DeviceAccess::Granted, &own, &[]),
            InputAccess::NoKeyboard
        );
    }
    #[test]
    fn wayland_setup_never_uses_xwayland_as_the_desktop_backend() {
        for name in ["GNOME", "KDE"] {
            let session = SessionInfo::from_env(
                |key| match key {
                    "XDG_SESSION_TYPE" => Some("wayland".into()),
                    "XDG_CURRENT_DESKTOP" => Some(name.into()),
                    _ => None,
                },
                "Linux",
            );
            assert!(backend_available(LayoutBackend::Auto, &session));
            assert!(!backend_available(LayoutBackend::X11, &session));
            assert!(!backend_available(LayoutBackend::Internal, &session));
            assert!(!backend_available(LayoutBackend::GnomeFallback, &session));
        }
    }
}

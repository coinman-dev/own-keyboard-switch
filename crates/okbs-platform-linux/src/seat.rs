//! logind boundary: input belongs only to the active local graphical session.
//! WSL exposes no host seat; isolated virtual-device tests use an explicit
//! development environment rather than assuming a Windows keyboard is present.
use crate::desktop::error;
use crate::session::SessionInfo;
use okbs_platform::Result;
use std::time::Duration;
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedObjectPath,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatState {
    pub active: bool,
    pub locked: bool,
}
impl Default for SeatState {
    fn default() -> Self {
        Self {
            active: false,
            locked: true,
        }
    }
}
#[derive(Debug)]
pub struct Seat {
    bus: Option<Connection>,
    session: Option<OwnedObjectPath>,
    development: bool,
}
impl Seat {
    pub fn connect() -> Self {
        let development = SessionInfo::detect().wsl;
        let bus = if development {
            None
        } else {
            zbus::blocking::connection::Builder::system()
                .ok()
                .and_then(|builder| {
                    builder
                        .method_timeout(Duration::from_millis(100))
                        .build()
                        .ok()
                })
        };
        Self {
            bus,
            session: None,
            development,
        }
    }
    pub fn state(&mut self) -> SeatState {
        if self.development {
            return SeatState {
                active: true,
                locked: false,
            };
        }
        match self.read() {
            Ok(state) => state,
            Err(_) => {
                self.session = None;
                SeatState::default()
            }
        }
    }
    fn read(&mut self) -> Result<SeatState> {
        if self.bus.is_none() {
            self.bus = zbus::blocking::connection::Builder::system()
                .map_err(error)?
                .method_timeout(Duration::from_millis(100))
                .build()
                .ok();
        }
        let bus = self
            .bus
            .as_ref()
            .ok_or_else(|| error("logind system bus is unavailable"))?;
        if self.session.is_none() {
            let manager = Proxy::new(
                bus,
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
            )
            .map_err(error)?;
            let direct =
                manager.call::<_, _, OwnedObjectPath>("GetSessionByPID", &(std::process::id(),));
            self.session = match direct {
                Ok(path) => Some(path),
                Err(_) => {
                    // Autostart from systemd --user may live outside session.scope.
                    let status = std::fs::read_to_string("/proc/self/status").map_err(error)?;
                    let uid: u32 = status
                        .lines()
                        .find_map(|line| line.strip_prefix("Uid:\t"))
                        .and_then(|line| line.split_whitespace().next())
                        .ok_or_else(|| error("cannot determine session UID"))?
                        .parse()
                        .map_err(error)?;
                    let user: OwnedObjectPath = manager.call("GetUser", &(uid,)).map_err(error)?;
                    let proxy = Proxy::new(
                        bus,
                        "org.freedesktop.login1",
                        user.as_str(),
                        "org.freedesktop.login1.User",
                    )
                    .map_err(error)?;
                    let (_, path): (String, OwnedObjectPath) =
                        proxy.get_property("Display").map_err(error)?;
                    if path.as_str() == "/" {
                        return Err(error("no graphical display session"));
                    }
                    Some(path)
                }
            };
        }
        let path = self
            .session
            .as_ref()
            .ok_or_else(|| error("no session identity"))?;
        let session = Proxy::new(
            bus,
            "org.freedesktop.login1",
            path.as_str(),
            "org.freedesktop.login1.Session",
        )
        .map_err(error)?;
        let kind: String = session.get_property("Type").map_err(error)?;
        let remote: bool = session.get_property("Remote").map_err(error)?;
        let active: bool = session.get_property("Active").map_err(error)?;
        let locked: bool = session.get_property("LockedHint").map_err(error)?;
        Ok(classify(&kind, remote, active, locked))
    }
}
fn classify(kind: &str, remote: bool, active: bool, locked: bool) -> SeatState {
    SeatState {
        active: active && !remote && matches!(kind, "x11" | "wayland"),
        locked,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn console_remote_and_inactive_sessions_are_never_capture_targets() {
        for kind in ["tty", "unspecified", "unknown"] {
            assert!(!classify(kind, false, true, false).active);
        }
        for kind in ["x11", "wayland"] {
            assert!(classify(kind, false, true, false).active);
            assert!(!classify(kind, true, true, false).active);
            assert!(!classify(kind, false, false, false).active);
            assert!(classify(kind, false, true, true).locked);
        }
    }
    struct Manager;
    #[zbus::interface(name = "org.freedesktop.login1.Manager")]
    impl Manager {
        #[zbus(name = "GetSessionByPID")]
        fn get_session_by_pid(&self, _pid: u32) -> OwnedObjectPath {
            OwnedObjectPath::try_from("/org/freedesktop/login1/session/fixture").unwrap()
        }
    }
    struct Session {
        state: std::sync::Arc<std::sync::Mutex<SeatState>>,
    }
    #[zbus::interface(name = "org.freedesktop.login1.Session")]
    impl Session {
        #[zbus(property, name = "Type")]
        fn session_type(&self) -> &str {
            "wayland"
        }
        #[zbus(property)]
        fn remote(&self) -> bool {
            false
        }
        #[zbus(property)]
        fn active(&self) -> bool {
            self.state.lock().unwrap().active
        }
        #[zbus(property)]
        fn locked_hint(&self) -> bool {
            self.state.lock().unwrap().locked
        }
    }
    #[test]
    #[ignore = "requires isolated D-Bus session; uses a private logind fixture"]
    fn logind_properties_are_read_and_missing_service_fails_closed() {
        let state = std::sync::Arc::new(std::sync::Mutex::new(SeatState {
            active: true,
            locked: false,
        }));
        let server = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name("org.freedesktop.login1")
            .unwrap()
            .serve_at("/org/freedesktop/login1", Manager)
            .unwrap()
            .serve_at(
                "/org/freedesktop/login1/session/fixture",
                Session {
                    state: state.clone(),
                },
            )
            .unwrap()
            .build()
            .unwrap();
        let client = zbus::blocking::connection::Builder::session()
            .unwrap()
            .method_timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let mut seat = Seat {
            bus: Some(client),
            session: None,
            development: false,
        };
        assert_eq!(
            seat.state(),
            SeatState {
                active: true,
                locked: false
            }
        );
        *state.lock().unwrap() = SeatState {
            active: false,
            locked: true,
        };
        assert_eq!(
            seat.state(),
            SeatState {
                active: false,
                locked: true
            }
        );
        drop(server);
        assert_eq!(seat.state(), SeatState::default());
    }
}

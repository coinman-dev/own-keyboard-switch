//! One-shot administrative action. The GUI and keyboard engine never become
//! root; the helper installs only the bundled rule and loads input modules.
#![allow(unsafe_code)]
use crate::desktop::error;
use okbs_platform::Result;
use std::process::{Child, Command, ExitStatus};

pub fn is_root() -> bool {
    // SAFETY: geteuid takes no pointers and has no side effects.
    unsafe { libc::geteuid() == 0 }
}

/// Keeps the UI responsive while the system authorization agent is open.
#[derive(Debug)]
pub struct PendingRequest {
    child: Child,
}
impl PendingRequest {
    pub fn start() -> Result<Self> {
        let image = std::env::var_os("APPIMAGE");
        let exe = image
            .clone()
            .map(std::path::PathBuf::from)
            .map(Ok)
            .unwrap_or_else(std::env::current_exe)
            .map_err(error)?;
        let mut command = Command::new("pkexec");
        command.arg(exe);
        if image.is_some() {
            command.arg("--appimage-extract-and-run");
        }
        command.arg("--setup-linux-input-helper");
        Ok(Self {
            child: command.spawn().map_err(error)?,
        })
    }
    pub fn poll(&mut self) -> Result<Option<()>> {
        self.child
            .try_wait()
            .map_err(error)?
            .map(outcome)
            .transpose()
    }
}
impl Drop for PendingRequest {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) && self.child.kill().is_ok() {
            let _ = self.child.wait();
        }
    }
}
fn outcome(status: ExitStatus) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(error("input setup was cancelled or failed"))
    }
}

pub fn request() -> Result<()> {
    let mut request = PendingRequest::start()?;
    outcome(request.child.wait().map_err(error)?)
}
pub fn install() -> Result<()> {
    if !is_root() {
        return Err(error(
            "the input setup helper requires administrator authorization",
        ));
    }
    let directory = std::path::Path::new("/usr/lib/udev/rules.d");
    std::fs::create_dir_all(directory).map_err(error)?;
    // Do not follow a pre-existing link when replacing a root-owned rule.
    let mut rule = tempfile::NamedTempFile::new_in(directory).map_err(error)?;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    rule.write_all(include_bytes!(
        "../../../packaging/linux/70-okbswitch.rules"
    ))
    .map_err(error)?;
    rule.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o644))
        .map_err(error)?;
    rule.as_file().sync_all().map_err(error)?;
    rule.persist(directory.join("70-okbswitch.rules"))
        .map_err(error)?;
    for (program, arguments) in [
        ("/usr/sbin/modprobe", vec!["evdev"]),
        ("/usr/sbin/modprobe", vec!["uinput"]),
        ("/usr/bin/udevadm", vec!["control", "--reload-rules"]),
        (
            "/usr/bin/udevadm",
            vec!["trigger", "--subsystem-match=input"],
        ),
        (
            "/usr/bin/udevadm",
            vec![
                "trigger",
                "--subsystem-match=misc",
                "--sysname-match=uinput",
            ],
        ),
        ("/usr/bin/udevadm", vec!["settle", "--timeout=5"]),
    ] {
        if !Command::new(program)
            .args(arguments)
            .status()
            .map_err(error)?
            .success()
        {
            return Err(error(format!("input setup command failed: {program}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authorization_failure_is_reported_and_pending_request_can_be_cancelled() {
        // Exercise process ownership without invoking pkexec or changing rules.
        let child = Command::new("sh").args(["-c", "exit 126"]).spawn().unwrap();
        let mut request = PendingRequest { child };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match request.poll() {
                Ok(None) => {}
                Err(_) => break,
                Ok(Some(())) => panic!("cancelled authorization was reported as success"),
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let pending = PendingRequest { child };
        let start = std::time::Instant::now();
        drop(pending);
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
}

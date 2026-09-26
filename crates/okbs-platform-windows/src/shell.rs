//! «Открыть» and «Сохранить как» dialogs, and system settings opened from
//! the tray menu.
#![allow(unsafe_code)]

use crate::hook::os_error;
use okbs_platform::{FileDialogs, FileRequest, PlatformError, Result, SystemSettings};
use std::path::PathBuf;
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND, LPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance,
    CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FILEOPENDIALOGOPTIONS, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT,
    FOS_PATHMUSTEXIST, FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog,
    IFileSaveDialog, SIGDN_FILESYSPATH, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SW_SHOWNORMAL,
};
use windows::core::{HRESULT, HSTRING, Interface, PCWSTR, w};

/// Windows Settings page listing the languages and their keyboard layouts.
const KEYBOARD_SETTINGS: &str = "ms-settings:regionlanguage";

/// `ShellExecuteW` reports failure as a value of 32 or less.
const SHELL_EXECUTE_MIN_SUCCESS: isize = 32;

/// Dialogs of the Windows shell (`IFileOpenDialog`, `IFileSaveDialog`).
#[derive(Debug, Default)]
pub struct WinFileDialogs;

/// Pages of the Windows Settings app.
#[derive(Debug, Default)]
pub struct WinSystemSettings;

/// COM for the calling thread, released when dropped.
struct Apartment(bool);

impl Apartment {
    fn enter() -> Self {
        // SAFETY: initializes COM for this thread only; balanced in Drop.
        let result =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        Self(result.is_ok())
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: pairs the successful CoInitializeEx of this thread.
            unsafe { CoUninitialize() };
        }
    }
}

struct Search<'a> {
    title: &'a str,
    process: u32,
    found: Option<HWND>,
}

unsafe extern "system" fn find(window: HWND, param: LPARAM) -> windows::core::BOOL {
    // SAFETY: `own_window` keeps Search alive for the synchronous enumeration.
    let search = unsafe { &mut *(param.0 as *mut Search<'_>) };
    let mut process = 0;
    // SAFETY: `process` is a valid out pointer; the handle comes from EnumWindows.
    unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
    // SAFETY: an opaque handle from EnumWindows.
    if process != search.process || !unsafe { IsWindowVisible(window) }.as_bool() {
        return true.into();
    }
    let mut text = [0u16; 256];
    // SAFETY: writable fixed-size buffer.
    let length = unsafe { GetWindowTextW(window, &mut text) };
    let length = usize::try_from(length).unwrap_or(0).min(text.len());
    if String::from_utf16_lossy(&text[..length]) == search.title {
        search.found = Some(window);
        return false.into();
    }
    true.into()
}

/// A visible window of this process with `title`; never another program's.
fn own_window(title: &str) -> Option<HWND> {
    let mut search = Search {
        title,
        process: std::process::id(),
        found: None,
    };
    // SAFETY: enumeration is synchronous and `search` outlives it. Stopping
    // early makes EnumWindows report an error, which is expected here.
    let _ = unsafe { EnumWindows(Some(find), LPARAM(&raw mut search as isize)) };
    search.found
}

fn show(request: &FileRequest, save: bool) -> Result<Option<PathBuf>> {
    let _apartment = Apartment::enter();
    let kind = HSTRING::from(format!("{} (*.{})", request.kind, request.extension));
    let spec = HSTRING::from(format!("*.{}", request.extension));
    let extension = HSTRING::from(request.extension.as_str());
    let title = HSTRING::from(request.title.as_str());
    let file_name = HSTRING::from(request.file_name.as_str());
    let filters = [COMDLG_FILTERSPEC {
        pszName: PCWSTR(kind.as_ptr()),
        pszSpec: PCWSTR(spec.as_ptr()),
    }];
    // SAFETY: COM calls in this thread's apartment; every string outlives
    // the dialog, and the returned path is freed with CoTaskMemFree.
    unsafe {
        let dialog: IFileDialog = if save {
            CoCreateInstance::<_, IFileSaveDialog>(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)
                .and_then(|dialog| dialog.cast())
        } else {
            CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                .and_then(|dialog| dialog.cast())
        }
        .map_err(|err| os_error("CoCreateInstance(FileDialog)", &err))?;
        let setup = || -> windows::core::Result<()> {
            dialog.SetFileTypes(&filters)?;
            dialog.SetDefaultExtension(PCWSTR(extension.as_ptr()))?;
            dialog.SetTitle(PCWSTR(title.as_ptr()))?;
            if save && !request.file_name.is_empty() {
                dialog.SetFileName(PCWSTR(file_name.as_ptr()))?;
            }
            let extra = if save {
                FOS_OVERWRITEPROMPT
            } else {
                FOS_FILEMUSTEXIST
            };
            let options = dialog.GetOptions()?;
            dialog.SetOptions(FILEOPENDIALOGOPTIONS(
                options.0 | FOS_FORCEFILESYSTEM.0 | FOS_PATHMUSTEXIST.0 | extra.0,
            ))
        };
        setup().map_err(|err| os_error("IFileDialog setup", &err))?;
        if let Err(err) = dialog.Show(own_window(&request.owner)) {
            if err.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Ok(None);
            }
            return Err(os_error("IFileDialog::Show", &err));
        }
        let item = dialog
            .GetResult()
            .map_err(|err| os_error("IFileDialog::GetResult", &err))?;
        let name = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|err| os_error("IShellItem::GetDisplayName", &err))?;
        let path = name.to_string();
        CoTaskMemFree(Some(name.0 as *const _));
        path.map(|path| Some(PathBuf::from(path)))
            .map_err(|err| PlatformError::Other(format!("file path: {err}")))
    }
}

impl FileDialogs for WinFileDialogs {
    fn open(&self, request: &FileRequest) -> Result<Option<PathBuf>> {
        show(request, false)
    }

    fn save(&self, request: &FileRequest) -> Result<Option<PathBuf>> {
        show(request, true)
    }
}

impl SystemSettings for WinSystemSettings {
    fn open_keyboard_settings(&self) -> Result<()> {
        let page = HSTRING::from(KEYBOARD_SETTINGS);
        // SAFETY: NUL-terminated strings that live until the call returns.
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(page.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize > SHELL_EXECUTE_MIN_SUCCESS {
            Ok(())
        } else {
            Err(PlatformError::Os {
                code: result.0 as isize as i64,
                context: format!("ShellExecuteW({KEYBOARD_SETTINGS})"),
            })
        }
    }
}

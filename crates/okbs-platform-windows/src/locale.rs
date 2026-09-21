//! Language of the current user's Windows interface, independent of keyboard layout.
#![allow(unsafe_code)]

use okbs_core::Lang;
use windows::Win32::Globalization::GetUserDefaultUILanguage;

/// Russian Windows uses Russian; other display languages fall back to English.
/// Uses <https://learn.microsoft.com/windows/win32/api/winnls/nf-winnls-getuserdefaultuilanguage>.
pub fn ui_language() -> Lang {
    // SAFETY: the function takes no pointers and returns a language identifier.
    let id = unsafe { GetUserDefaultUILanguage() };
    Lang::from_windows_langid(id).unwrap_or(Lang::En)
}

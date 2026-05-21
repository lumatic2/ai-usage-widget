// Focus an existing IDE/editor window whose title contains the project folder name.
// Cursor/VSCode/JetBrains all include the project root in their window titles.

use std::path::Path;

pub fn focus_for_cwd(cwd: &str) -> bool {
    let project_name = match Path::new(cwd).file_name().and_then(|n| n.to_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return false,
    };
    focus_impl(&project_name)
}

#[cfg(windows)]
fn focus_impl(project_name: &str) -> bool {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
        SetForegroundWindow, ShowWindow, GW_OWNER, SW_RESTORE,
    };

    struct Ctx {
        needle: String,
        best: Option<HWND>,
        best_score: usize,
    }

    extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return TRUE;
            }
            // Skip owned windows (dialogs, tooltips).
            if GetWindow(hwnd, GW_OWNER).is_ok() {
                let owner = GetWindow(hwnd, GW_OWNER).ok();
                if let Some(o) = owner {
                    if o.0 != std::ptr::null_mut() {
                        return TRUE;
                    }
                }
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return TRUE;
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let copied = GetWindowTextW(hwnd, &mut buf);
            if copied <= 0 {
                return TRUE;
            }
            let title = String::from_utf16_lossy(&buf[..copied as usize]);
            let title_lc = title.to_lowercase();
            let needle_lc = ctx.needle.to_lowercase();
            if !title_lc.contains(&needle_lc) {
                return TRUE;
            }
            // Score: prefer titles that look like editor/IDE titles.
            let mut score = 1;
            for hint in [
                "visual studio code",
                "cursor",
                "intellij",
                "pycharm",
                "webstorm",
                "rustrover",
                "goland",
                "sublime",
                "windsurf",
            ] {
                if title_lc.contains(hint) {
                    score = 10;
                    break;
                }
            }
            if score > ctx.best_score {
                ctx.best_score = score;
                ctx.best = Some(hwnd);
            }
        }
        TRUE
    }

    let mut ctx = Ctx {
        needle: project_name.to_string(),
        best: None,
        best_score: 0,
    };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    if let Some(hwnd) = ctx.best {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
        return true;
    }
    false
}

#[cfg(not(windows))]
fn focus_impl(_project_name: &str) -> bool {
    false
}

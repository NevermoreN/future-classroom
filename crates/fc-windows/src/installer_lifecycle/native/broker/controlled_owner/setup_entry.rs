//! Local GUI intent selection. No UI value authorizes MSI access. The selection
//! dialog is fully destroyed before the 120-second native coordinator begins;
//! no live UI message loop is blocked and no CA/G thread is added.
use super::super::super::super::broker::{self as entry, SetupCommand};
use super::*;
use windows::Win32::{System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*};

const INSTALL: u16 = 101;
const REPAIR: u16 = 102;
const UNINSTALL: u16 = 103;
const CANCEL: u16 = 2;

// Windows targets have at least 32-bit pointers; these u16 dialog IDs fit
// isize exactly. Rust also supports narrower non-Windows targets, so it does
// not provide a general From<u16> implementation for isize.
fn choice(value: isize) -> Result<Option<SetupCommand>> {
    match value {
        value if value == INSTALL as isize => Ok(Some(SetupCommand::Install)),
        value if value == REPAIR as isize => Ok(Some(SetupCommand::Repair)),
        value if value == UNINSTALL as isize => Ok(Some(SetupCommand::Uninstall)),
        value if value == CANCEL as isize => Ok(None),
        _ => Err(LifecycleError::Invalid),
    }
}
fn dword(words: &mut Vec<u16>, value: u32) {
    words.extend([value as u16, (value >> 16) as u16]);
}
fn text(words: &mut Vec<u16>, value: &str) {
    words.extend(value.encode_utf16());
    words.push(0);
}
fn item(words: &mut Vec<u16>, id: u16, class: u16, x: u16, y: u16, width: u16, title: &str) {
    if !words.len().is_multiple_of(2) {
        words.push(0);
    }
    let mut style = WS_CHILD.0 | WS_VISIBLE.0;
    if class == 0x80 {
        style |= WS_TABSTOP.0;
        if id == CANCEL {
            style |= BS_DEFPUSHBUTTON as u32;
        }
    }
    dword(words, style);
    dword(words, 0);
    words.extend([x, y, width, 20, id, 0xffff, class]);
    text(words, title);
    words.push(0); // no creation data
}
fn template() -> Vec<u32> {
    let mut words = Vec::with_capacity(256);
    dword(
        &mut words,
        WS_POPUP.0
            | WS_CAPTION.0
            | WS_SYSMENU.0
            | DS_MODALFRAME as u32
            | DS_SETFONT as u32
            | DS_CENTER as u32,
    );
    dword(&mut words, 0);
    words.extend([5, 0, 0, 304, 82, 0, 0]); // cdit/x/y/cx/cy/menu/class
    text(&mut words, "未来教室安装");
    words.push(9);
    text(&mut words, "Microsoft YaHei");
    item(
        &mut words,
        104,
        0x82,
        12,
        12,
        280,
        "请选择操作，或取消退出。",
    );
    for (id, x, title) in [
        (INSTALL, 12, "安装"),
        (REPAIR, 84, "修复"),
        (UNINSTALL, 156, "卸载"),
        (CANCEL, 228, "取消"),
    ] {
        item(&mut words, id, 0x80, x, 48, 64, title);
    }
    if !words.len().is_multiple_of(2) {
        words.push(0);
    }
    // DLGTEMPLATE/each item requires DWORD alignment. Vec<u32> guarantees that
    // alignment while preserving the fixed little-endian Windows word layout.
    words
        .chunks_exact(2)
        .map(|pair| u32::from(pair[0]) | (u32::from(pair[1]) << 16))
        .collect()
}
unsafe extern "system" fn dialog(hwnd: HWND, message: u32, w: WPARAM, _: LPARAM) -> isize {
    match message {
        WM_INITDIALOG => 1,
        WM_CLOSE => {
            let _ = unsafe { EndDialog(hwnd, CANCEL as isize) };
            1
        }
        WM_COMMAND if w.0 >> 16 == 0 => {
            let id = w.0 as u16;
            if matches!(id, INSTALL | REPAIR | UNINSTALL | CANCEL) {
                let _ = unsafe { EndDialog(hwnd, id as isize) };
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}
pub(crate) fn interactive() -> Result<u32> {
    // Reject a bare/legacy helper before displaying Setup. This read-only image
    // discovery acquires no gate, Pending or MSI transaction. Return all pins
    // before the unbounded human selection wait. The chosen request then gets
    // exactly its one original BROKER_BUDGET; run_setup reopens/rechecks its
    // sealed image and fixed source, so this UI check grants no authority.
    let mut image = Session::new(Instant::now() + entry::BROKER_BUDGET)?;
    image.sealed_package_sha.ok_or(LifecycleError::Invalid)?;
    image.resources.release()?;
    drop(image);
    let template = template();
    let module = unsafe { GetModuleHandleW(None)? };
    let selected = unsafe {
        DialogBoxIndirectParamW(
            Some(HINSTANCE(module.0)),
            template.as_ptr().cast(),
            None,
            Some(dialog),
            LPARAM(0),
        )
    };
    let Some(command) = choice(selected)? else {
        return Ok(ERROR_INSTALL_USEREXIT.0);
    };
    // DialogBox has ended and destroyed its window. No UI object is waiting for
    // messages while the original-thread bounded coordinator synchronously runs.
    let result = entry::run_setup(command);
    let (message, icon) = match &result {
        Ok(0) => ("操作已完成。".to_owned(), MB_ICONINFORMATION),
        Ok(code) => (format!("操作未完成（返回代码 {code}）。"), MB_ICONERROR),
        Err(_) => (
            "操作未完成。请检查当前安装状态，或联系管理员。".to_owned(),
            MB_ICONERROR,
        ),
    };
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            windows::core::w!("未来教室安装"),
            MB_OK | icon,
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup_dialog_values_only_select_closed_requests_or_cancel() {
        assert_eq!(choice(INSTALL as isize), Ok(Some(SetupCommand::Install)));
        assert_eq!(choice(REPAIR as isize), Ok(Some(SetupCommand::Repair)));
        assert_eq!(
            choice(UNINSTALL as isize),
            Ok(Some(SetupCommand::Uninstall))
        );
        assert_eq!(choice(CANCEL as isize), Ok(None));
        for value in [-1, 0, 1, 3, 100, 104, 65537] {
            assert_eq!(choice(value), Err(LifecycleError::Invalid));
        }
    }
}

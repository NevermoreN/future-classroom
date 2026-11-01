//! Explicit, synchronous startup failure feedback for an interactive caller.
//! Merely loading this module never creates a window or changes console state.

/// Reports whether the calling process currently has a console window handle.
/// A pseudoconsole can return a message-only HWND, so `true` does not imply a
/// visible console. `false` is only this call's absence of an associated HWND.
/// No console is created, attached, detached, or otherwise modified.
#[cfg(windows)]
pub fn console_window_present() -> bool {
    // SAFETY: GetConsoleWindow takes no arguments and returns a borrowed HWND.
    // This observation neither owns nor closes the returned window.
    let console = unsafe { windows::Win32::System::Console::GetConsoleWindow() };
    !console.0.is_null()
}

/// Displays an OK-only error dialog. The caller decides whether its launch mode
/// is interactive; autostart and diagnostic paths must not call this implicitly.
///
/// Message text is limited to 2048 UTF-16 code units plus its terminator. Embedded
/// NULs become visible `␀` characters, and truncation preserves Unicode scalars.
/// This modal call does not create a worker, attach a console, or replace standard
/// handles. It does not request topmost or foreground activation privileges.
#[cfg(windows)]
pub fn show_startup_failure(message: &str) -> windows::core::Result<()> {
    use windows::{
        core::{w, Error, PCWSTR},
        Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
    };

    let message = encode_message(message);
    // SAFETY: the bounded, NUL-terminated text remains alive throughout this
    // synchronous call; the title is a static NUL-terminated literal. No HWND or
    // externally owned resource is borrowed or transferred.
    let result = unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            w!("未来教室"),
            MB_OK | MB_ICONERROR,
        )
    };
    if result.0 == 0 {
        return Err(Error::from_thread());
    }
    Ok(())
}

#[cfg(any(windows, test))]
const MAX_MESSAGE_UNITS: usize = 2048;
#[cfg(any(windows, test))]
const TRUNCATION: &str = "…（已截断）";

#[cfg(any(windows, test))]
fn encode_message(message: &str) -> Vec<u16> {
    let message = if message.is_empty() {
        "启动失败，请重新启动或联系管理员。"
    } else {
        message
    };
    let mut encoded = Vec::with_capacity(MAX_MESSAGE_UNITS + 1);
    for scalar in message.chars() {
        let scalar = if scalar == '\0' { '␀' } else { scalar };
        if encoded.len() + scalar.len_utf16() > MAX_MESSAGE_UNITS {
            let suffix_units = TRUNCATION.encode_utf16().count();
            while encoded.len() + suffix_units > MAX_MESSAGE_UNITS {
                if encoded
                    .pop()
                    .is_some_and(|last| (0xdc00..=0xdfff).contains(&last))
                {
                    let _ = encoded.pop(); // Remove the matching high surrogate as well.
                }
            }
            encoded.extend(TRUNCATION.encode_utf16());
            break;
        }
        let mut units = [0; 2];
        encoded.extend_from_slice(scalar.encode_utf16(&mut units));
    }
    encoded.push(0);
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(encoded: &[u16]) -> String {
        assert_eq!(encoded.last(), Some(&0));
        assert!(!encoded[..encoded.len() - 1].contains(&0));
        String::from_utf16(&encoded[..encoded.len() - 1]).expect("complete Unicode scalars")
    }

    #[test]
    fn startup_message_preserves_chinese_and_supplementary_characters() {
        let original = "无法启动未来教室：路径含中文 🏫。\n请重新打开。";
        assert_eq!(text(&encode_message(original)), original);
    }

    #[test]
    fn startup_message_does_not_hide_text_after_embedded_nul() {
        assert_eq!(text(&encode_message("启动\0失败\0详情")), "启动␀失败␀详情");
        assert!(!text(&encode_message("")).is_empty());
    }

    #[test]
    fn startup_message_exact_bound_is_preserved_and_overflow_is_marked() {
        let exact = "界".repeat(MAX_MESSAGE_UNITS);
        assert_eq!(text(&encode_message(&exact)), exact);
        let overflow = encode_message(&(exact + "超出"));
        assert!(overflow.len() <= MAX_MESSAGE_UNITS + 1);
        assert!(text(&overflow).ends_with(TRUNCATION));
    }

    #[test]
    fn startup_message_truncation_never_exposes_a_partial_surrogate_pair() {
        for prefix_units in [0, 1, MAX_MESSAGE_UNITS - 1] {
            let long = "a".repeat(prefix_units) + &"🏫".repeat(MAX_MESSAGE_UNITS);
            let encoded = encode_message(&long);
            assert!(encoded.len() <= MAX_MESSAGE_UNITS + 1);
            assert!(encoded.capacity() <= MAX_MESSAGE_UNITS + 1);
            let decoded = text(&encoded);
            assert!(decoded.ends_with(TRUNCATION));
            assert!(!decoded.contains('\u{fffd}'));
        }
    }
}

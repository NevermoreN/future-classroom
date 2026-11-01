//! UI-thread-owned common colour dialog; never holds a form RefCell borrow.
//! Contract: https://learn.microsoft.com/windows/win32/api/commdlg/ns-commdlg-choosecolorw
use fc_domain::identity::Rgb;
use windows::{
    core::{Error, Result, HRESULT},
    Win32::{
        Foundation::{COLORREF, HWND},
        UI::Controls::Dialogs::{
            ChooseColorW, CommDlgExtendedError, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW,
        },
    },
};
fn encode(rgb: Rgb) -> COLORREF {
    COLORREF(u32::from(rgb.red) | (u32::from(rgb.green) << 8) | (u32::from(rgb.blue) << 16))
}
fn decode(rgb: COLORREF) -> Rgb {
    Rgb {
        red: rgb.0 as u8,
        green: (rgb.0 >> 8) as u8,
        blue: (rgb.0 >> 16) as u8,
    }
}
pub(super) fn choose(owner: HWND, initial: Rgb) -> Result<Option<Rgb>> {
    // Exactly sixteen cells, owned for the full synchronous modal lifetime.
    // Custom swatches are deliberately ephemeral and are not application settings.
    let mut custom = [encode(initial); 16];
    let mut dialog = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner,
        rgbResult: encode(initial),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_RGBINIT | CC_FULLOPEN,
        ..Default::default()
    };
    // SAFETY: live caller-owned HWND, complete structure and sixteen mutable
    // COLORREF cells all outlive this modal call. No State/RefCell borrow spans it.
    if unsafe { ChooseColorW(&mut dialog) }.as_bool() {
        return Ok(Some(decode(dialog.rgbResult)));
    }
    // A false result with error zero is user cancellation; rgbResult is then
    // undefined by the contract and must never be copied into the draft.
    // SAFETY: immediate thread-local diagnostic of the just-returned common dialog.
    let error = unsafe { CommDlgExtendedError() }.0;
    if error == 0 {
        Ok(None)
    } else {
        Err(Error::from_hresult(HRESULT(0x80004005u32 as i32)))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_rgb_colorref_roundtrip_has_no_alpha_or_byte_order_swap() {
        for rgb in [
            Rgb {
                red: 0,
                green: 0,
                blue: 0,
            },
            Rgb {
                red: 255,
                green: 255,
                blue: 255,
            },
            Rgb {
                red: 0x12,
                green: 0x34,
                blue: 0xAB,
            },
        ] {
            assert_eq!(decode(encode(rgb)), rgb);
            assert_eq!(encode(rgb).0 >> 24, 0);
        }
    }
}

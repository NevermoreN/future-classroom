//! Shared blue-and-white workbench colors, with the user's high-contrast palette.
use windows::Win32::{
    Foundation::COLORREF,
    Graphics::Gdi::{
        GetSysColor, COLOR_BTNFACE, COLOR_BTNTEXT, COLOR_GRAYTEXT, COLOR_HIGHLIGHT,
        COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWFRAME, COLOR_WINDOWTEXT,
    },
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        WindowsAndMessaging::{
            SystemParametersInfoW, SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        },
    },
};

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub background: COLORREF,
    pub surface: COLORREF,
    pub soft: COLORREF,
    pub primary: COLORREF,
    pub primary_hover: COLORREF,
    pub text: COLORREF,
    pub muted: COLORREF,
    pub border: COLORREF,
    pub selected: COLORREF,
    pub selected_text: COLORREF,
    pub disabled: COLORREF,
    pub on_primary: COLORREF,
}

pub(super) const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

pub(super) fn palette() -> Palette {
    let mut contrast = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: the synchronous system query writes only this initialized structure.
    let high_contrast = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some((&raw mut contrast).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
            && (contrast.dwFlags & HCF_HIGHCONTRASTON).0 != 0
    };
    if high_contrast {
        // SAFETY: GetSysColor is a read-only system palette query on Win7 and later.
        return unsafe {
            Palette {
                background: COLORREF(GetSysColor(COLOR_WINDOW)),
                surface: COLORREF(GetSysColor(COLOR_WINDOW)),
                soft: COLORREF(GetSysColor(COLOR_BTNFACE)),
                primary: COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                primary_hover: COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                text: COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                muted: COLORREF(GetSysColor(COLOR_BTNTEXT)),
                border: COLORREF(GetSysColor(COLOR_WINDOWFRAME)),
                selected: COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                selected_text: COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)),
                disabled: COLORREF(GetSysColor(COLOR_GRAYTEXT)),
                on_primary: COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)),
            }
        };
    }
    Palette {
        background: rgb(246, 249, 255),
        surface: rgb(255, 255, 255),
        soft: rgb(246, 249, 255),
        primary: rgb(8, 123, 255),
        primary_hover: rgb(0, 104, 229),
        text: rgb(16, 26, 61),
        muted: rgb(83, 101, 140),
        border: rgb(223, 232, 247),
        selected: rgb(234, 243, 255),
        selected_text: rgb(0, 104, 229),
        disabled: rgb(128, 143, 170),
        on_primary: rgb(255, 255, 255),
    }
}

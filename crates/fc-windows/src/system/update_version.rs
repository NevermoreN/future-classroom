//! Read-only kernel version; independent of the executable compatibility manifest.
pub fn update_windows_baseline() -> Result<fc_domain::updates::WindowsBaseline, &'static str> {
    use fc_domain::updates::WindowsBaseline;
    use windows::{
        core::{w, PCSTR},
        Win32::System::{
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            SystemInformation::OSVERSIONINFOEXW,
        },
    };
    // SAFETY: ntdll is loaded for the entire process; its borrowed module handle
    // is never released. The documented RtlGetVersion ABI accepts this prefix.
    let module =
        unsafe { GetModuleHandleW(w!("ntdll.dll")) }.map_err(|_| "Windows version unavailable")?;
    let address = unsafe { GetProcAddress(module, PCSTR(c"RtlGetVersion".as_ptr().cast())) }
        .ok_or("Windows version unavailable")?;
    let read: unsafe extern "system" fn(*mut OSVERSIONINFOEXW) -> i32 =
        unsafe { std::mem::transmute(address) };
    let mut version = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOEXW>() as u32,
        ..Default::default()
    };
    if unsafe { read(&mut version) } != 0 || version.dwPlatformId != 2 || version.wProductType != 1
    {
        return Err("unsupported Windows baseline");
    }
    match (
        version.dwMajorVersion,
        version.dwMinorVersion,
        version.dwBuildNumber,
    ) {
        (6, 1, 7601) if version.wServicePackMajor >= 1 => Ok(WindowsBaseline::Windows7Sp1),
        (10, 0, build) if build >= 22000 => Ok(WindowsBaseline::Windows11),
        (10, 0, build) if build >= 10240 => Ok(WindowsBaseline::Windows10),
        _ => Err("unsupported Windows baseline"),
    }
}

//! COM task storage uses the documented system Ole32 allocator, including on
//! Win7. Resolving by name avoids a same-symbol MSVC import-library choice that
//! can otherwise bind a plain `#[link(name = "ole32")]` declaration to combase.
//!
//! One initialization permanently pins the already-loaded exact system Ole32.
//! This intentional, bounded process-lifetime pin makes every returned function
//! pointer valid through process exit; it is not a per-operation DLL reference.
//! No DLL is loaded, no search policy changes, and no allocation is attempted if
//! resolution fails. Failed initialization is cached too, with no replacement.
use std::{ffi::c_void, sync::OnceLock};
use windows::{
    core::{s, Error, Result, BOOL, HRESULT, PCSTR, PCWSTR},
    Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, E_ACCESSDENIED, E_NOTIMPL, HMODULE},
        Globalization::{CompareStringOrdinal, CSTR_EQUAL},
        System::{
            LibraryLoader::{
                GetModuleFileNameW, GetModuleHandleExW, GetProcAddress,
                GET_MODULE_HANDLE_EX_FLAG_PIN,
            },
            SystemInformation::{
                GetNativeSystemInfo, GetSystemDirectoryW, GetSystemWow64DirectoryW,
                PROCESSOR_ARCHITECTURE_AMD64, PROCESSOR_ARCHITECTURE_INTEL, SYSTEM_INFO,
            },
            Threading::{GetCurrentProcess, IsWow64Process},
        },
    },
};

type FreeTaskMemory = unsafe extern "system" fn(*const c_void);

// Only a function pointer is shared. There are no Send/Sync handle wrappers or
// owner transmutations. Ole32 was pinned before this value became reachable.
static ALLOCATOR: OnceLock<std::result::Result<TaskMemory, HRESULT>> = OnceLock::new();
const PATH_UNITS: usize = 1024;
const OLE32: &[u16] = &[
    b'o' as u16,
    b'l' as u16,
    b'e' as u16,
    b'3' as u16,
    b'2' as u16,
    b'.' as u16,
    b'd' as u16,
    b'l' as u16,
    b'l' as u16,
];

#[derive(Clone, Copy)]
pub(crate) struct TaskMemory {
    free: FreeTaskMemory,
}
impl TaskMemory {
    /// # Safety
    /// `memory` is null or an allocation transferred by the COM task allocator,
    /// not yet freed. It must be freed exactly once and never used afterwards.
    pub(crate) unsafe fn free(self, memory: *const c_void) {
        // SAFETY: caller supplies the task allocation. Resolution checked the
        // exact export and ABI before the process-lifetime capability was made.
        unsafe { (self.free)(memory) };
    }
}

/// The operation (including any call that can return task storage) cannot begin
/// until the fallible release capability has been resolved successfully.
pub(crate) fn with_task_memory<T>(operation: impl FnOnce(TaskMemory) -> Result<T>) -> Result<T> {
    with_resolver(allocator, operation)
}

fn with_resolver<T>(
    resolve: impl FnOnce() -> Result<TaskMemory>,
    operation: impl FnOnce(TaskMemory) -> Result<T>,
) -> Result<T> {
    operation(resolve()?)
}

fn allocator() -> Result<TaskMemory> {
    ALLOCATOR
        .get_or_init(|| resolve().map_err(|error| error.code()))
        .as_ref()
        .copied()
        .map_err(|code| Error::from_hresult(*code))
}

struct ModulePath {
    units: [u16; PATH_UNITS],
    len: usize,
}
impl ModulePath {
    fn as_pcwstr(&self) -> PCWSTR {
        PCWSTR(self.units.as_ptr())
    }
    fn as_slice(&self) -> &[u16] {
        &self.units[..self.len]
    }
}

fn system_ole32_path() -> Result<ModulePath> {
    let mut system = SYSTEM_INFO::default();
    let mut wow64 = BOOL::default();
    // SAFETY: synchronous calls write initialized, correctly sized outputs;
    // GetCurrentProcess is a borrowed pseudo handle and is never closed here.
    unsafe {
        GetNativeSystemInfo(&mut system);
        IsWow64Process(GetCurrentProcess(), &mut wow64)?;
    }
    // IsWow64Process is ambiguous for some ARM emulation. ARM64 support is not a
    // confirmed project target; do not silently choose its wrong system folder.
    // SAFETY: GetNativeSystemInfo initializes the documented architecture union.
    let architecture = unsafe { system.Anonymous.Anonymous.wProcessorArchitecture };
    if architecture != PROCESSOR_ARCHITECTURE_INTEL && architecture != PROCESSOR_ARCHITECTURE_AMD64
    {
        return Err(Error::from_hresult(E_NOTIMPL));
    }
    let mut path = ModulePath {
        units: [0; PATH_UNITS],
        len: 0,
    };
    // SAFETY: fixed bounded output buffers. WOW64 uses its actual 32-bit system
    // directory, not an unredirected System32 string compared against SysWOW64.
    let len = unsafe {
        if wow64.as_bool() {
            GetSystemWow64DirectoryW(Some(&mut path.units))
        } else {
            GetSystemDirectoryW(Some(&mut path.units))
        }
    } as usize;
    if len == 0 {
        return Err(Error::from_thread());
    }
    if len >= PATH_UNITS {
        return Err(Error::from_hresult(ERROR_INSUFFICIENT_BUFFER.to_hresult()));
    }
    // System directories must be absolute, non-truncated and NUL terminated.
    // No environment variable, user path or caller-provided DLL name is used.
    if len < 3
        || path.units[1] != b':' as u16
        || path.units[2] != b'\\' as u16
        || path.units[..len].contains(&0)
        || path.units[len] != 0
    {
        return Err(Error::from_hresult(E_ACCESSDENIED));
    }
    path.len = len;
    if path.units[path.len - 1] != b'\\' as u16 {
        if path.len + 1 >= PATH_UNITS {
            return Err(Error::from_hresult(ERROR_INSUFFICIENT_BUFFER.to_hresult()));
        }
        path.units[path.len] = b'\\' as u16;
        path.len += 1;
    }
    if path.len + OLE32.len() >= PATH_UNITS {
        return Err(Error::from_hresult(ERROR_INSUFFICIENT_BUFFER.to_hresult()));
    }
    path.units[path.len..path.len + OLE32.len()].copy_from_slice(OLE32);
    path.len += OLE32.len();
    Ok(path)
}

fn loaded_path(module: HMODULE) -> Result<ModulePath> {
    let mut path = ModulePath {
        units: [0; PATH_UNITS],
        len: 0,
    };
    // SAFETY: the returned module has already been pinned, and the buffer is
    // fully owned. A full/truncated buffer is always rejected.
    let len = unsafe { GetModuleFileNameW(Some(module), &mut path.units) } as usize;
    if len == 0 {
        return Err(Error::from_thread());
    }
    if len >= PATH_UNITS {
        return Err(Error::from_hresult(ERROR_INSUFFICIENT_BUFFER.to_hresult()));
    }
    if path.units[..len].contains(&0) || path.units[len] != 0 {
        return Err(Error::from_hresult(E_ACCESSDENIED));
    }
    path.len = len;
    Ok(path)
}

fn same_system_path(expected: &[u16], actual: &[u16]) -> bool {
    // Case-insensitive Windows ordinal comparison, including Unicode paths. No
    // basename-only match, alias expansion or System32/SysWOW64 equivalence.
    if expected.is_empty() || expected.len() != actual.len() || expected.len() >= PATH_UNITS {
        return false;
    }
    // SAFETY: valid owned UTF-16 slices, explicit lengths bounded below i32::MAX,
    // and true is passed through the documented BOOL conversion. API failure (0)
    // is not CSTR_EQUAL and therefore cannot authorize a different module.
    unsafe { CompareStringOrdinal(expected, actual, true) == CSTR_EQUAL }
}

fn resolve() -> Result<TaskMemory> {
    let expected = system_ole32_path()?;
    let mut module = HMODULE::default();
    // SAFETY: the full OS-selected system path is bounded and NUL terminated.
    // This only retrieves an ALREADY LOADED module. Ole32 is a static dependency
    // of the existing COM calls. PIN holds it until process termination, including
    // on a later validation failure; OnceLock bounds this to one initialization.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_PIN,
            expected.as_pcwstr(),
            &mut module,
        )?
    };
    let actual = loaded_path(module)?;
    if !same_system_path(expected.as_slice(), actual.as_slice()) {
        return Err(Error::from_hresult(E_ACCESSDENIED));
    }
    resolve_export(module)
}

fn export(module: HMODULE, name: PCSTR) -> Result<unsafe extern "system" fn() -> isize> {
    // SAFETY: private callers pass the pinned module and a static NUL name. This
    // only retrieves an address, not a typed capability for arbitrary exports.
    unsafe { GetProcAddress(module, name) }.ok_or_else(Error::from_thread)
}

fn resolve_export(module: HMODULE) -> Result<TaskMemory> {
    let export = export(module, s!("CoTaskMemFree"))?;
    // SAFETY: production name is exactly documented CoTaskMemFree: WINAPI void
    // (LPVOID). `system` preserves stdcall on x86 and one pointer/void ABI. The
    // non-null export belongs to the pinned module (forwarders are loader-owned).
    let free = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, FreeTaskMemory>(export)
    };
    Ok(TaskMemory { free })
}

#[cfg(test)]
mod tests;

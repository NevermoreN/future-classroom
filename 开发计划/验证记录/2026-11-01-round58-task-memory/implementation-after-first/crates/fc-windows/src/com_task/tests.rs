use super::*;
use std::{cell::Cell, ptr};
use windows::Win32::System::LibraryLoader::GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;

type AllocTaskMemory = unsafe extern "system" fn(usize) -> *mut c_void;

fn current_module() -> HMODULE {
    let expected = system_ole32_path().expect("OS-selected system directory");
    let mut module = HMODULE::default();
    // SAFETY: initialization succeeded before this helper; its exact module is
    // pinned. This test borrows that reference and never decrements it.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            expected.as_pcwstr(),
            &mut module,
        )
        .expect("the exact pinned module remains loaded");
    }
    assert!(same_system_path(
        expected.as_slice(),
        loaded_path(module).unwrap().as_slice()
    ));
    module
}

fn allocation_export(module: HMODULE) -> AllocTaskMemory {
    // SAFETY: resolve from the same exact pinned Ole32, rather than generating a
    // possibly competing import for the test allocator. The documented ABI is
    // WINAPI LPVOID(size_t), including stdcall and 32-bit size_t on x86.
    let export = unsafe { GetProcAddress(module, s!("CoTaskMemAlloc")) }
        .expect("system Ole32 CoTaskMemAlloc export");
    unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, AllocTaskMemory>(export) }
}

#[test]
fn task_memory_real_system_ole32_allocate_free_and_null() {
    with_task_memory(|memory| {
        let module = current_module();
        let alloc = allocation_export(module);
        // SAFETY: real default COM task allocation; release capability already
        // exists and the module stays pinned. No generated task allocator import.
        unsafe {
            let storage = alloc(64).cast::<u8>();
            assert!(!storage.is_null());
            storage.write_bytes(0xa5, 64);
            assert_eq!(std::slice::from_raw_parts(storage, 64), &[0xa5; 64]);
            memory.free(storage.cast());
            memory.free(ptr::null());
        }
        Ok(())
    })
    .expect("real system Ole32 task allocator");
}

#[test]
fn task_memory_missing_export_rejects_before_any_task_allocation() {
    let memory = allocator().unwrap();
    let module = current_module();
    let allocated = Cell::new(0);
    let result = with_resolver(
        || {
            export(module, s!("FutureClassroom_R58_Absent_Task_Export"))?;
            Ok(memory)
        },
        |resolved| {
            allocated.set(allocated.get() + 1);
            let alloc = allocation_export(module);
            // SAFETY: this real allocation branch must not run on lookup failure.
            unsafe {
                let pointer = alloc(16);
                resolved.free(pointer);
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(allocated.get(), 0);
    // A deliberately missing test export did not alter the real once-resolved
    // process capability. It is still callable, not replaced by a test fake.
    unsafe { memory.free(ptr::null()) };
}

#[test]
fn task_memory_allocator_module_and_capability_remain_process_pinned() {
    let first = allocator().unwrap();
    let first_module = current_module();
    for _ in 0..4 {
        let next = allocator().unwrap();
        assert!(std::ptr::fn_addr_eq(first.free, next.free));
        assert_eq!(current_module(), first_module);
        // SAFETY: null is documented as a no-op. Cached function remains valid
        // between calls without per-call LoadLibrary/refcount/unload operations.
        unsafe { next.free(ptr::null()) };
    }
}

#[test]
fn task_memory_real_wow64_path_uses_the_matching_system_directory() {
    allocator().unwrap();
    let expected = system_ole32_path().unwrap();
    let module = current_module();
    let actual = loaded_path(module).unwrap();
    assert!(same_system_path(expected.as_slice(), actual.as_slice()));
    let mut wow64 = BOOL::default();
    // SAFETY: borrowed process pseudo handle and synchronous bool output.
    unsafe { IsWow64Process(GetCurrentProcess(), &mut wow64).unwrap() };
    let mut selected = [0u16; PATH_UNITS];
    // SAFETY: owned bounded directory output, matching the actual process mode.
    let len = unsafe {
        if wow64.as_bool() {
            GetSystemWow64DirectoryW(Some(&mut selected))
        } else {
            GetSystemDirectoryW(Some(&mut selected))
        }
    } as usize;
    assert!(len > 0 && len < PATH_UNITS);
    let expected_directory_len = expected.len - OLE32.len() - 1;
    // API documents a possible trailing slash only for a root directory.
    let directory_len = if selected[len - 1] == b'\\' as u16 {
        len - 1
    } else {
        len
    };
    assert!(same_system_path(
        &selected[..directory_len],
        &expected.as_slice()[..expected_directory_len]
    ));
    eprintln!(
        "R58_TASK_MEMORY_SYSTEM_MODULE wow64={} exact_path=true pinned=true allocation_abi=system",
        wow64.as_bool()
    );
}

#[test]
fn task_memory_path_check_rejects_wrong_folder_prefix_and_module() {
    let wide = |value: &str| value.encode_utf16().collect::<Vec<_>>();
    let expected = wide("C:\\Windows\\SysWOW64\\Ole32.dll");
    assert!(same_system_path(
        &expected,
        &wide("c:\\WINDOWS\\syswow64\\ole32.DLL")
    ));
    for wrong in [
        "C:\\Windows\\System32\\Ole32.dll",
        "C:\\Windows\\SysWOW64\\combase.dll",
        "C:\\Users\\Synthetic\\Ole32.dll",
        "Ole32.dll",
        "C:\\Windows\\SysWOW64\\Ole32.dll.extra",
    ] {
        assert!(!same_system_path(&expected, &wide(wrong)), "{wrong}");
    }
}

#[test]
fn task_memory_system_path_uses_windows_unicode_case_rules() {
    let upper = "C:\\WÜNDOWS\\System32\\Ole32.dll"
        .encode_utf16()
        .collect::<Vec<_>>();
    let lower = "c:\\wündows\\system32\\ole32.DLL"
        .encode_utf16()
        .collect::<Vec<_>>();
    assert!(same_system_path(&upper, &lower));
    assert!(!same_system_path(&upper, &[]));
    assert!(!same_system_path(
        &[b'x' as u16; PATH_UNITS],
        &[b'x' as u16; PATH_UNITS]
    ));
}

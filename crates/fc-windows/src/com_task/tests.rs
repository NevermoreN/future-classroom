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
        nt_system_path(&expected).unwrap().as_slice(),
        mapped_path(module).unwrap().as_slice()
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
    // Borrow the statically loaded Ole32 for compact diagnostics before the
    // allocator's original OnceLock/PIN. No load/refcount change or extra PIN.
    let expected = system_ole32_path().unwrap();
    let expected_nt = nt_system_path(&expected);
    let mut module = HMODULE::default();
    let lookup = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            expected.as_pcwstr(),
            &mut module,
        )
    };
    eprintln!(
        "R58_TASK_MEMORY_SYSTEM_LOOKUP expected_dos_utf16={:?} unref_result={:?}",
        expected.as_slice(),
        lookup.as_ref().map(|_| ()).map_err(|error| error.code().0)
    );
    lookup.unwrap();
    let loader_spelling = loaded_path(module);
    let actual_nt = mapped_path(module);
    eprintln!(
        "R58_TASK_MEMORY_MAPPED_IDENTITY expected_nt_utf16={:?} actual_nt_utf16={:?} loader_utf16={:?}",
        expected_nt.as_ref().map(|path| path.as_slice()).map_err(|error| error.code().0),
        actual_nt.as_ref().map(|path| path.as_slice()).map_err(|error| error.code().0),
        loader_spelling.as_ref().map(|path| path.as_slice()).map_err(|error| error.code().0)
    );
    allocator().unwrap();
    assert!(same_system_path(
        expected_nt.unwrap().as_slice(),
        actual_nt.unwrap().as_slice()
    ));
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

#[test]
fn task_memory_current_device_mapping_binds_exact_mapped_image_not_loader_spelling() {
    let wide = |value: &str| value.encode_utf16().collect::<Vec<_>>();
    let dos = wide("C:\\Windows\\SysWOW64\\ole32.dll");
    let current_and_prior = wide("\\Device\\HarddiskVolume3\0\\Device\\HarddiskVolume8\0\0");
    let expected = nt_path_from_current_mapping(&dos, &current_and_prior).unwrap();
    assert!(same_system_path(
        expected.as_slice(),
        &wide("\\Device\\HarddiskVolume3\\Windows\\SysWOW64\\ole32.dll")
    ));
    // Loader spelling can be System32 on a real WOW64 load. It never authorizes
    // the image; the mapped NT object must match the actual OS-selected folder.
    for wrong in [
        "\\Device\\HarddiskVolume3\\Windows\\System32\\ole32.dll",
        "\\Device\\HarddiskVolume8\\Windows\\SysWOW64\\ole32.dll",
        "\\Device\\HarddiskVolume3\\Windows\\SysWOW64\\combase.dll",
        "\\Device\\HarddiskVolume3\\Users\\Synthetic\\ole32.dll",
    ] {
        assert!(
            !same_system_path(expected.as_slice(), &wide(wrong)),
            "{wrong}"
        );
    }
    let native = nt_path_from_current_mapping(
        &wide("C:\\Windows\\System32\\ole32.dll"),
        &current_and_prior,
    )
    .unwrap();
    assert!(!same_system_path(expected.as_slice(), native.as_slice()));
    // The same helper used by the final live requery rejects a changed current
    // drive mapping even when its prior mappings include the original volume.
    let changed = nt_path_from_current_mapping(
        &dos,
        &wide("\\Device\\HarddiskVolume8\0\\Device\\HarddiskVolume3\0\0"),
    )
    .unwrap();
    assert!(!same_system_path(expected.as_slice(), changed.as_slice()));
}

#[test]
fn task_memory_device_mapping_metadata_is_bounded_and_fail_closed() {
    let wide = |value: &str| value.encode_utf16().collect::<Vec<_>>();
    let dos = wide("C:\\Windows\\SysWOW64\\ole32.dll");
    for invalid in [
        Vec::new(),
        wide("\0\0"),
        wide("\\Device\\HarddiskVolume3"),
        wide("C:\\Synthetic\0\0"),
        wide("\\Device\\\0\0"),
        wide("\\Device\\HarddiskVolume3\\\0\0"),
    ] {
        assert!(nt_path_from_current_mapping(&dos, &invalid).is_err());
    }
    let valid_mapping = wide("\\Device\\HarddiskVolume3\0\0");
    assert!(
        nt_path_from_current_mapping(&wide("C:\\Windows\0\\ole32.dll"), &valid_mapping).is_err()
    );
    assert!(nt_path_from_current_mapping(&wide("ole32.dll"), &valid_mapping).is_err());
    let mut oversized = wide("C:\\");
    oversized.extend([b'x' as u16; PATH_UNITS]);
    assert_eq!(
        nt_path_from_current_mapping(&oversized, &valid_mapping)
            .err()
            .unwrap()
            .code(),
        ERROR_INSUFFICIENT_BUFFER.to_hresult()
    );
}

//! Fixed current-account installed-state discovery; no registry/path input.
use super::*;
pub(super) fn capture(
    resources: &Resources,
    source: &source_package::CapturedSource,
    deadline: Instant,
) -> Result<Option<Registration>> {
    let check = || {
        current(deadline)?;
        resources.check_live()
    };
    check()?;
    let mut found = BTreeSet::new();
    let family = HSTRING::from(format!("{{{INSTALL_FAMILY}}}"));
    for index in 0..=16 {
        check()?;
        let mut product = [0u16; 39];
        let rc =
            unsafe { MsiEnumRelatedProductsW(&family, None, index, PWSTR(product.as_mut_ptr())) };
        check()?;
        if rc == ERROR_NO_MORE_ITEMS.0 {
            break;
        }
        if rc != 0 || index == 16 {
            return Err(LifecycleError::Invalid);
        }
        if !found.insert(guid(&product)?) {
            return Err(LifecycleError::Unsettled);
        }
    }
    if found.is_empty() {
        return Ok(None);
    }
    if found.len() != 1 || !found.contains(&source.manifest.product) {
        return Err(LifecycleError::Invalid);
    }
    let product = HSTRING::from(format!("{{{}}}", source.manifest.product));
    let sid = format::sid_text(&resources.sid).ok_or(LifecycleError::Invalid)?;
    for index in 0..=3 {
        check()?;
        let mut name = [0u16; 39];
        let mut context = MSIINSTALLCONTEXT_NONE;
        let mut actual_sid = [0u16; 184];
        let mut len = actual_sid.len() as u32;
        let rc = unsafe {
            MsiEnumProductsExW(
                &product,
                None,
                MSIINSTALLCONTEXT_ALL.0 as u32,
                index,
                Some(&mut name),
                Some(&mut context),
                Some(PWSTR(actual_sid.as_mut_ptr())),
                Some(&mut len),
            )
        };
        check()?;
        if rc == ERROR_NO_MORE_ITEMS.0 {
            if index != 1 {
                return Err(LifecycleError::Invalid);
            }
            break;
        }
        if rc != 0
            || index != 0
            || context != MSIINSTALLCONTEXT_USERUNMANAGED
            || guid(&name)? != source.manifest.product
            || len as usize >= actual_sid.len()
            || String::from_utf16(&actual_sid[..len as usize])
                .map_err(|_| LifecycleError::Invalid)?
                != sid
        {
            return Err(LifecycleError::Invalid);
        }
    }
    let property = |name: &str| -> Result<String> {
        check()?;
        let mut b = [0u16; PATH_LIMIT];
        let mut n = b.len() as u32;
        let rc = unsafe {
            MsiGetProductInfoExW(
                &product,
                None,
                MSIINSTALLCONTEXT_USERUNMANAGED,
                &HSTRING::from(name),
                Some(PWSTR(b.as_mut_ptr())),
                Some(&mut n),
            )
        };
        check()?;
        if rc != 0 || n as usize >= b.len() || b[n as usize] != 0 {
            return Err(LifecycleError::Invalid);
        }
        let text = String::from_utf16(&b[..n as usize]).map_err(|_| LifecycleError::Invalid)?;
        if text.chars().any(|c| c.is_control()) {
            return Err(LifecycleError::Invalid);
        }
        Ok(text)
    };
    if property("State")? != "5"
        || property("VersionString")? != source.manifest.version
        || !property("Transforms")?.is_empty()
    {
        return Err(LifecycleError::Invalid);
    }
    check()?;
    let patches = unsafe {
        MsiEnumPatchesExW(
            &product,
            None,
            MSIINSTALLCONTEXT_ALL.0 as u32,
            MSIPATCHSTATE_ALL.0 as u32,
            0,
            None,
            None,
            None,
            None,
            None,
        )
    };
    check()?;
    if patches != ERROR_NO_MORE_ITEMS.0 {
        return Err(LifecycleError::Invalid);
    }
    let cache = property("LocalPackage")?;
    let mut windows = [0u16; PATH_LIMIT];
    let n = unsafe { GetWindowsDirectoryW(Some(&mut windows)) } as usize;
    if n == 0 || n >= windows.len() {
        return Err(LifecycleError::Invalid);
    }
    let windows = String::from_utf16(&windows[..n]).map_err(|_| LifecycleError::Invalid)?;
    if !format::cache_path(&windows, &cache) {
        return Err(LifecycleError::Invalid);
    }
    check()?;
    Ok(Some(Registration {
        product: source.manifest.product.clone(),
        version: source.manifest.version.clone(),
        cache: PathBuf::from(cache),
    }))
}
fn guid(b: &[u16; 39]) -> Result<String> {
    if b[38] != 0 {
        return Err(LifecycleError::Invalid);
    }
    format::product_code(&String::from_utf16(&b[..38]).map_err(|_| LifecycleError::Invalid)?)
        .ok_or(LifecycleError::Invalid)
}

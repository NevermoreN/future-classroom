//! Only this native local picker may create an external-file selection.
use super::files::LockedFile;
use std::path::{Path, PathBuf};
use windows::{
    core::{w, Error, Result},
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::Shell::{Common::COMDLG_FILTERSPEC, *},
    },
};
#[link(name = "ole32")]
unsafe extern "system" {
    #[link_name = "CoTaskMemFree"]
    fn free_task_memory(memory: *const core::ffi::c_void);
}
pub struct SelectedFile {
    path: PathBuf,
}
impl SelectedFile {
    pub fn allocation_bytes(&self) -> usize {
        self.path.as_os_str().len().saturating_mul(2)
    }
    pub fn display_name(&self) -> Result<String> {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned)
            .ok_or_else(|| Error::from_hresult(E_INVALIDARG))
    }
    pub fn open(self, max: u64) -> Result<LockedFile> {
        LockedFile::open(&self.path, max)
    }
    /// Hidden diagnostics use only synthetic files under a protected isolated
    /// deployment's quarantine, never the user's own material or production tree.
    pub fn isolated_probe(root: &Path, name: &str) -> Result<Self> {
        if root
            .file_name()
            .and_then(|n| n.to_str())
            .is_none_or(|n| !n.starts_with("FutureClassroom-Test-"))
            || name.is_empty()
            || name.len() > 80
            || name.contains(['\\', '/', ':', '\0'])
            || name == "."
            || name == ".."
        {
            return Err(Error::from_hresult(E_ACCESSDENIED));
        }
        Ok(Self {
            path: root.join("quarantine").join(name),
        })
    }
}
pub fn select(owner: Option<HWND>) -> Result<Option<SelectedFile>> {
    // SAFETY: called on the UI STA. No path has been trusted or opened here;
    // worker handle validation follows before copying any bytes. The native modal
    // loop continues routing WTS/power to the independent memory-only host control.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(
            dialog.GetOptions()?
                | FOS_FORCEFILESYSTEM
                | FOS_FILEMUSTEXIST
                | FOS_PATHMUSTEXIST
                | FOS_NOCHANGEDIR
                | FOS_NODEREFERENCELINKS,
        )?;
        dialog.SetTitle(w!("添加课堂资料"))?;
        let filters = [COMDLG_FILTERSPEC {
            pszName: w!("课堂资料（图片、文本、PDF、Office）"),
            pszSpec: w!("*.png;*.jpg;*.jpeg;*.bmp;*.txt;*.pdf;*.pptx;*.docx;*.xlsx"),
        }];
        dialog.SetFileTypes(&filters)?;
        match dialog.Show(owner) {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => return Ok(None),
            Err(e) => return Err(e),
        }
        let item = dialog.GetResult()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH)?;
        let value = p.to_string();
        free_task_memory(p.0.cast());
        let path = PathBuf::from(value?);
        // Force drive-letter local validation again on the worker; picker options
        // alone are not a network/reparse/type guarantee.
        Ok(Some(SelectedFile { path }))
    }
}

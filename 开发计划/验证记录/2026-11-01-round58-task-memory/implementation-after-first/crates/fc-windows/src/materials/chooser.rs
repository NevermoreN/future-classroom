//! Only this native local picker may create an external-file selection.
mod backup_destination;
mod validation;
pub use backup_destination::{BackupCopyReceipt, BackupDestination};

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
    /// Display text only, never a filesystem capability or an authorization
    /// decision. All byte access still consumes this selection through `open`.
    pub fn display_path(&self) -> Result<String> {
        self.path
            .to_str()
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
    crate::com_task::with_task_memory(|memory| unsafe {
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
        memory.free(p.0.cast());
        let path = PathBuf::from(value?);
        // Force drive-letter local validation again on the worker; picker options
        // alone are not a network/reparse/type guarantee.
        Ok(Some(SelectedFile { path }))
    })
}

pub fn select_content_txt(owner: Option<HWND>) -> Result<Option<SelectedFile>> {
    Ok(select_content(owner, false)?.pop())
}
pub fn select_content_images(owner: Option<HWND>) -> Result<Vec<SelectedFile>> {
    select_content(owner, true)
}
fn select_content(owner: Option<HWND>, images: bool) -> Result<Vec<SelectedFile>> {
    // SAFETY: own UI STA picker; all external paths remain opaque selections.
    // A bounded worker pins/verifies every source again after the dialog closes.
    crate::com_task::with_task_memory(|memory| unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let mut options = dialog.GetOptions()?
            | FOS_FORCEFILESYSTEM
            | FOS_FILEMUSTEXIST
            | FOS_PATHMUSTEXIST
            | FOS_NOCHANGEDIR
            | FOS_NODEREFERENCELINKS;
        if images {
            options |= FOS_ALLOWMULTISELECT;
        }
        dialog.SetOptions(options)?;
        let filter = if images {
            dialog.SetTitle(w!("选择图片（最多50张，可在导入前调整顺序）"))?;
            COMDLG_FILTERSPEC {
                pszName: w!("图片（PNG、JPEG、BMP）"),
                pszSpec: w!("*.png;*.jpg;*.jpeg;*.bmp"),
            }
        } else {
            dialog.SetTitle(w!("导入独立文字（UTF-8 或 UTF-16 文本）"))?;
            COMDLG_FILTERSPEC {
                pszName: w!("文本（TXT）"),
                pszSpec: w!("*.txt"),
            }
        };
        dialog.SetFileTypes(&[filter])?;
        match dialog.Show(owner) {
            Ok(()) => (),
            Err(error) if error.code() == ERROR_CANCELLED.to_hresult() => return Ok(Vec::new()),
            Err(error) => return Err(error),
        }
        let items = dialog.GetResults()?;
        let count = items.GetCount()?;
        if count == 0 || count > if images { 50 } else { 1 } {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let mut files = Vec::with_capacity(count as usize);
        for index in 0..count {
            let item = items.GetItemAt(index)?;
            let text = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let value = text.to_string();
            memory.free(text.0.cast());
            files.push(SelectedFile {
                path: PathBuf::from(value?),
            });
        }
        Ok(files)
    })
}

/// Native picker only; no external package bytes are opened on the UI thread.
pub fn select_backup(owner: Option<HWND>) -> Result<Option<SelectedFile>> {
    // SAFETY: called on the UI STA, same COM lifetime and cancellation contract
    // as the material picker. Worker validation establishes actual file trust.
    crate::com_task::with_task_memory(|memory| unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(
            dialog.GetOptions()?
                | FOS_FORCEFILESYSTEM
                | FOS_FILEMUSTEXIST
                | FOS_PATHMUSTEXIST
                | FOS_NOCHANGEDIR
                | FOS_NODEREFERENCELINKS
                | FOS_STRICTFILETYPES
                | FOS_DONTADDTORECENT,
        )?;
        dialog.SetTitle(w!("选择未来教室完整备份"))?;
        dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
            pszName: w!("未来教室完整备份（FCBAK）"),
            pszSpec: w!("*.fcbak"),
        }])?;
        match dialog.Show(owner) {
            Ok(()) => (),
            Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => return Ok(None),
            Err(e) => return Err(e),
        }
        let text = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let value = text.to_string();
        memory.free(text.0.cast());
        let path = PathBuf::from(value?);
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(validation::backup_name)
        {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        Ok(Some(SelectedFile { path }))
    })
}
/// Selection only: returns an opaque untrusted target, creates no files and
/// changes no current directory. Existing targets are never overwritten later.
pub fn save_backup(owner: Option<HWND>, suggested_name: &str) -> Result<Option<BackupDestination>> {
    if !validation::backup_name(suggested_name) {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    // SAFETY: UI STA owns the modal dialog. HSTRINGs live through synchronous
    // calls; the returned filesystem name still requires worker handle checks.
    crate::com_task::with_task_memory(|memory| unsafe {
        let dialog: IFileSaveDialog =
            CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(
            dialog.GetOptions()?
                | FOS_FORCEFILESYSTEM
                | FOS_PATHMUSTEXIST
                | FOS_NOCHANGEDIR
                | FOS_NODEREFERENCELINKS
                | FOS_STRICTFILETYPES
                | FOS_DONTADDTORECENT,
        )?;
        dialog.SetTitle(w!("另存未来教室完整备份（不覆盖已有文件）"))?;
        dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
            pszName: w!("未来教室完整备份（FCBAK）"),
            pszSpec: w!("*.fcbak"),
        }])?;
        dialog.SetDefaultExtension(w!("fcbak"))?;
        dialog.SetFileName(&windows::core::HSTRING::from(suggested_name))?;
        match dialog.Show(owner) {
            Ok(()) => (),
            Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => return Ok(None),
            Err(e) => return Err(e),
        }
        let text = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let value = text.to_string();
        memory.free(text.0.cast());
        BackupDestination::from_picker(PathBuf::from(value?)).map(Some)
    })
}

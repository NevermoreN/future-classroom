//! One owned PDF validator subprocess, constrained before it reads any document.
//! It never renders or starts a third-party application.
use std::{
    io::{Read, Write},
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use windows::{
    core::{Error, Result},
    Win32::{
        Foundation::*,
        System::{JobObjects::*, Threading::*},
    },
};
pub const PDF_MEMORY_BYTES: usize = 80 * 1024 * 1024;
struct Job(HANDLE);
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct OwnedChild(Child);
impl std::ops::Deref for OwnedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Every error/cancellation path drains this one owned child. Never leave
        // an unjoined helper or release its worker slot while it can still parse.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[derive(Debug)]
pub struct HelperReport {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub peak_private_bytes: usize,
}
pub fn validate_pdf(path: &Path, canceled: impl Fn() -> bool) -> Result<HelperReport> {
    let job = Job(unsafe { CreateJobObjectW(None, None)? });
    let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
            PerProcessUserTimeLimit: 100_000_000,
            ActiveProcessLimit: 1,
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_PROCESS_TIME
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            ..Default::default()
        },
        ProcessMemoryLimit: PDF_MEMORY_BYTES,
        ..Default::default()
    };
    unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as _,
            std::mem::size_of_val(&limits) as u32,
        )?;
    }
    let mut child = OwnedChild(
        Command::new(std::env::current_exe()?)
            .arg("--material-pdf-helper")
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW.0)
            .spawn()?,
    );
    // Child's first operation is reading one byte from stdin. It cannot inspect
    // PDF bytes until we assign this job, including memory/CPU/descendant caps.
    if let Err(e) = unsafe { AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())) } {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e);
    }
    let start = Instant::now();
    let release = child
        .stdin
        .take()
        .ok_or_else(Error::from_thread)?
        .write_all(b"G");
    if release.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::from_hresult(E_FAIL));
    }
    let status = loop {
        if canceled() || start.elapsed() >= Duration::from_secs(30) {
            unsafe {
                let _ = TerminateJobObject(job.0, 3);
            }
            let _ = child.wait();
            return Err(Error::from_hresult(ERROR_CANCELLED.to_hresult()));
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stdout = Vec::new();
    if let Some(out) = child.stdout.take() {
        out.take(4097).read_to_end(&mut stdout)?;
    }
    if stdout.len() > 4096 {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let mut observed = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            Some(job.0),
            JobObjectExtendedLimitInformation,
            &mut observed as *mut _ as _,
            std::mem::size_of_val(&observed) as u32,
            None,
        )?;
    }
    Ok(HelperReport {
        exit_code: status.code(),
        stdout,
        peak_private_bytes: observed.PeakProcessMemoryUsed,
    })
}

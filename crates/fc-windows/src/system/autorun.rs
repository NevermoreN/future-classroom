//! Fixed HKCU Run adapter. It is not exposed as an HTTP command executor.
use std::{
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
};
use windows::{
    core::{w, Error, Result, HSTRING},
    Win32::{Foundation::*, System::Registry::*},
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const MAX_COMMAND_CHARS: usize = 260;

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: exclusively owned successful registry open/create result.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct AutostartReadback {
    pub present: bool,
    pub matches_expected: bool,
    pub actual_command: Option<String>,
}

pub struct RunAtLogin {
    key: String,
    executable: PathBuf,
    command: String,
    _thread: PhantomData<Rc<()>>,
}
impl RunAtLogin {
    /// Only the local composition root may construct this adapter. The executable
    /// is always the running program; teacher data cannot select another target.
    pub fn current_executable() -> Result<Self> {
        let executable = std::env::current_exe().map_err(|_| Error::from_hresult(E_FAIL))?;
        Self::build(RUN_KEY.into(), executable)
    }
    fn build(key: String, executable: PathBuf) -> Result<Self> {
        let command = command_for(&executable)?;
        Ok(Self {
            key,
            executable,
            command,
            _thread: PhantomData,
        })
    }
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    pub fn expected_command(&self) -> &str {
        &self.command
    }
    pub fn read_back(&self) -> Result<AutostartReadback> {
        let name = HSTRING::from(self.key.as_str());
        let mut key = HKEY::default();
        // SAFETY: registry strings live for synchronous call; result is owned below.
        let code =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &name, None, KEY_QUERY_VALUE, &mut key) };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(self.observed(None));
        }
        code.ok()?;
        self.read_key(&Key(key))
    }
    pub fn apply(&self, enabled: bool) -> Result<AutostartReadback> {
        let name = HSTRING::from(self.key.as_str());
        let mut key = HKEY::default();
        // SAFETY: fixed current-user project value; no elevation/system-wide writes.
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &name,
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
            .ok()?;
        }
        let key = Key(key);
        if enabled {
            let bytes: Vec<u8> = self
                .command
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect();
            // SAFETY: bounded owned REG_SZ bytes, including UTF-16 terminator.
            unsafe {
                RegSetValueExW(key.0, w!("FutureClassroom"), None, REG_SZ, Some(&bytes)).ok()?;
            }
        } else {
            // SAFETY: delete only the fixed project value; absence is idempotent.
            let result = unsafe { RegDeleteValueW(key.0, w!("FutureClassroom")) };
            if result != ERROR_FILE_NOT_FOUND {
                result.ok()?;
            }
        }
        let actual = self.read_key(&key)?;
        if (enabled && !actual.matches_expected) || (!enabled && actual.present) {
            return Err(Error::from_hresult(E_UNEXPECTED));
        }
        Ok(actual)
    }
    fn observed(&self, actual: Option<String>) -> AutostartReadback {
        AutostartReadback {
            present: actual.is_some(),
            matches_expected: actual.as_ref() == Some(&self.command),
            actual_command: actual,
        }
    }
    fn read_key(&self, key: &Key) -> Result<AutostartReadback> {
        // Allocate a fixed bound before reading; oversized/policy-corrupted values
        // report an error rather than allocating based on registry input.
        let mut data = [0u8; (MAX_COMMAND_CHARS + 1) * 2];
        let mut bytes = data.len() as u32;
        let mut kind = REG_VALUE_TYPE::default();
        // SAFETY: all output pointers refer to live bounded storage for this call.
        let code = unsafe {
            RegQueryValueExW(
                key.0,
                w!("FutureClassroom"),
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut bytes),
            )
        };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(self.observed(None));
        }
        code.ok()?;
        if kind != REG_SZ || bytes < 2 || bytes as usize > data.len() || !bytes.is_multiple_of(2) {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let mut units: Vec<u16> = data[..bytes as usize]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        if units.pop() != Some(0) || units.contains(&0) {
            return Err(Error::from_hresult(E_INVALIDARG));
        }
        let actual = String::from_utf16(&units).map_err(|_| Error::from_hresult(E_INVALIDARG))?;
        Ok(self.observed(Some(actual)))
    }
}

fn command_for(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::from_hresult(E_INVALIDARG))?;
    let raw = text.as_bytes();
    // Reject UNC/device paths, drive-relative paths, ADS, quotes and controls.
    if !path.is_absolute()
        || raw.len() < 4
        || !raw[0].is_ascii_alphabetic()
        || raw[1] != b':'
        || raw[2] != b'\\'
        || text[2..].contains(':')
        || text.chars().any(|c| c == '"' || c.is_control())
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_none_or(|e| !e.eq_ignore_ascii_case("exe"))
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    let command = format!("\"{text}\" --autostart");
    if command.encode_utf16().count() > MAX_COMMAND_CHARS {
        return Err(Error::from_hresult(E_INVALIDARG));
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Scratch(String);
    impl Drop for Scratch {
        fn drop(&mut self) {
            // SAFETY: exclusively generated test key; test value removed first.
            unsafe {
                let path = HSTRING::from(self.0.as_str());
                let mut key = HKEY::default();
                if RegOpenKeyExW(HKEY_CURRENT_USER, &path, None, KEY_SET_VALUE, &mut key).is_ok() {
                    let key = Key(key);
                    let _ = RegDeleteValueW(key.0, w!("FutureClassroom"));
                }
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &path);
            }
        }
    }
    #[test]
    fn quoted_absolute_command_and_exclusive_test_registry_roundtrip() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Single leaf directly in Software: cleanup cannot leave intermediate keys.
        let scratch = Scratch(format!(
            r"Software\FutureClassroom-TestRun-{}-{nonce}",
            std::process::id()
        ));
        let run = RunAtLogin::build(
            scratch.0.clone(),
            PathBuf::from(r"C:\合成 测试\future-classroom.exe"),
        )
        .unwrap();
        assert_eq!(
            run.expected_command(),
            "\"C:\\合成 测试\\future-classroom.exe\" --autostart"
        );
        assert!(!run.read_back().unwrap().present);
        assert!(run.apply(true).unwrap().matches_expected);
        assert!(run.read_back().unwrap().matches_expected);
        let moved = RunAtLogin::build(
            scratch.0.clone(),
            PathBuf::from(r"C:\moved\future-classroom.exe"),
        )
        .unwrap();
        let stale = moved.read_back().unwrap();
        assert!(stale.present);
        assert!(!stale.matches_expected); // detection never silently repairs OS policy/path
        assert!(!run.apply(false).unwrap().present);
        drop(scratch);
        assert!(!run.read_back().unwrap().present);
        for invalid in [
            r"app.exe",
            r"C:app.exe",
            r"\\server\share\app.exe",
            r"C:\bad.exe:stream",
            "C:\\bad\".exe",
            r"C:\a\..\b.exe",
        ] {
            assert!(command_for(Path::new(invalid)).is_err(), "{invalid}");
        }
    }
}

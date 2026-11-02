//! Pure bounds for native observation, without paths or hashes granting writes.

pub(super) fn product_code(value: &str) -> Option<String> {
    let inner = value.strip_prefix('{')?.strip_suffix('}')?;
    if inner.len() != 36
        || !inner.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'A'..=b'F').contains(&b)
            }
        })
        || inner.bytes().all(|b| b == b'0' || b == b'-')
    {
        return None;
    }
    Some(inner.to_owned())
}

pub(super) fn version(value: &str) -> Option<[u32; 3]> {
    let mut fields = value.split('.');
    let mut parsed = [0; 3];
    for (field, limit) in parsed.iter_mut().zip([255, 255, 65535]) {
        let text = fields.next()?;
        if text.is_empty()
            || text.len() > 5
            || !text.bytes().all(|b| b.is_ascii_digit())
            || (text.len() != 1 && text.starts_with('0'))
        {
            return None;
        }
        *field = text.parse().ok()?;
        if *field > limit {
            return None;
        }
    }
    fields.next().is_none().then_some(parsed)
}

pub(super) fn sid_text(sid: &[u8]) -> Option<String> {
    if sid.len() < 8 || sid[0] != 1 || sid[1] > 15 || sid.len() != 8 + 4 * usize::from(sid[1]) {
        return None;
    }
    let authority = sid[2..8].iter().fold(0u64, |n, b| (n << 8) | u64::from(*b));
    let mut text = format!("S-1-{authority}");
    for part in sid[8..].as_chunks::<4>().0 {
        text.push('-');
        text.push_str(&u32::from_le_bytes(*part).to_string());
    }
    Some(text)
}

/// Lexical comparison against the native OS Windows directory. Never resolve a
/// caller path. Pins/reparse/link/ACL/native object checks are separate.
pub(super) fn cache_path(windows: &str, actual: &str) -> bool {
    let bytes = windows.as_bytes();
    if bytes.len() < 4
        || !bytes[0].is_ascii_alphabetic()
        || &bytes[1..3] != b":\\"
        || windows.ends_with('\\')
        || windows.contains(['/', '\0', '\r', '\n'])
        || windows
            .split('\\')
            .skip(1)
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return false;
    }
    let prefix = format!("{windows}\\Installer\\");
    let Some((head, leaf)) = actual.split_at_checked(prefix.len()) else {
        return false;
    };
    if !head.eq_ignore_ascii_case(&prefix) || leaf.len() > 132 {
        return false;
    }
    let Some((name, extension)) = leaf.rsplit_once('.') else {
        return false;
    };
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        && extension.eq_ignore_ascii_case("msi")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_identity_is_one_canonical_nonzero_braced_guid() {
        assert_eq!(
            product_code("{9E50831D-7811-5039-8BC3-4934647448D8}").unwrap(),
            "9E50831D-7811-5039-8BC3-4934647448D8"
        );
        for text in [
            "9E50831D-7811-5039-8BC3-4934647448D8",
            "{9e50831d-7811-5039-8bc3-4934647448d8}",
            "{00000000-0000-0000-0000-000000000000}",
            "{9E50831D-7811-5039-8BC3-4934647448D8}\0",
        ] {
            assert!(product_code(text).is_none());
        }
    }

    #[test]
    fn version_has_three_numeric_fields_with_msi_limits_and_no_aliases() {
        assert_eq!(version("255.255.65535"), Some([255, 255, 65535]));
        assert_eq!(version("0.1.0"), Some([0, 1, 0]));
        for text in [
            "1.2",
            "1.2.3.4",
            "256.0.0",
            "0.256.0",
            "0.0.65536",
            "01.2.3",
            "1.2.-1",
            "1.2.3 ",
            "1..3",
        ] {
            assert!(version(text).is_none(), "{text}");
        }
    }

    #[test]
    fn sid_uses_binary_authority_and_exact_little_endian_subauthorities() {
        let sid = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];
        assert_eq!(sid_text(&sid).as_deref(), Some("S-1-5-32-544"));
        for n in 0..sid.len() {
            assert!(sid_text(&sid[..n]).is_none());
        }
        let mut wrong = sid;
        wrong[0] = 2;
        assert!(sid_text(&wrong).is_none());
        wrong[0] = 1;
        wrong[1] = 16;
        assert!(sid_text(&wrong).is_none());
    }

    #[test]
    fn localpackage_is_fixed_native_windows_installer_direct_leaf() {
        assert!(cache_path(r"C:\Windows", r"c:\WINDOWS\Installer\20DDA.msi"));
        assert!(cache_path(
            r"D:\Windows",
            r"D:\Windows\Installer\native-cache_1.MSI"
        ));
        for actual in [
            r"C:\Windows\Installer2\a.msi",
            r"C:\Windows\Installer\..\a.msi",
            r"C:\Windows\Installer\sub\a.msi",
            r"C:\Windows\Installer\a.msi:stream",
            r"\\?\C:\Windows\Installer\a.msi",
            "C:/Windows/Installer/a.msi",
            r"C:\Windows\Installer\.msi",
            r"C:\Windows\Installer\a.msi ",
            r"D:\Windows\Installer\a.msi",
        ] {
            assert!(!cache_path(r"C:\Windows", actual), "{actual}");
        }
        assert!(!cache_path(
            r"C:\Windows\..",
            r"C:\Windows\..\Installer\a.msi"
        ));
    }
}

//! Pure read bounds and one-attempt native close accounting, never authority.
pub(super) const ROW_LIMIT: usize = 72;
pub(super) const DIRECTORY_LIMIT: usize = 72;
pub(super) const CUSTOM_ACTION_LIMIT: usize = 16;
pub(super) const EXECUTE_SEQUENCE_LIMIT: usize = 64;
pub(super) const BINARY_LIMIT: usize = 32;
pub(super) const TEXT_LIMIT: usize = 512;
pub(super) const RECEIPT_LIMIT: usize = 256;

pub(super) fn room(kernel: usize, msi: usize, incoming: usize) -> bool {
    kernel
        .checked_add(msi)
        .and_then(|n| n.checked_add(incoming))
        .is_some_and(|n| n <= 64)
}
pub(super) fn text(value: &str) -> bool {
    value.encode_utf16().count() <= TEXT_LIMIT && !value.chars().any(char::is_control)
}

/// A close receipt is reserved before each MSI acquisition. At a known read
/// bound failure every already-owned MSI object still has its own close slot.
/// Pure row counts/closed receipts are not live kernel/MSI capabilities.
pub(super) fn receipt_room(closed: usize, live_msi: usize, incoming: usize) -> bool {
    closed
        .checked_add(live_msi)
        .and_then(|n| n.checked_add(incoming))
        .is_some_and(|n| n <= RECEIPT_LIMIT)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DirectoryRow {
    pub(super) directory: String,
    pub(super) parent: Option<String>,
    pub(super) default_dir: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CustomActionRow {
    pub(super) action: String,
    pub(super) kind: i32,
    pub(super) source: Option<String>,
    pub(super) target: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ExecuteSequenceRow {
    pub(super) action: String,
    pub(super) condition: Option<String>,
    pub(super) sequence: Option<i32>,
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 72
        && (value.as_bytes()[0].is_ascii_alphabetic() || value.as_bytes()[0] == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
}
fn segment(value: &str) -> bool {
    let base = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    !value.is_empty()
        && value.is_ascii()
        && value.encode_utf16().count() <= 255
        && !value.ends_with(['.', ' '])
        && !value
            .chars()
            .any(|c| c.is_control() || r#"\/:|<>"?*"#.contains(c))
        && !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}
/// Declared relative directory facts only. No filesystem lookup, identity,
/// published-predecessor claim or maintenance permit follows from these rows.
/// Deliberately supports the closed long-name-only builder form; MSI short-name
/// aliases/source-name syntax and alternate roots are not silently normalized.
pub(super) fn declared_directory_paths(
    rows: &[DirectoryRow],
) -> Option<std::collections::BTreeMap<String, String>> {
    use std::collections::{BTreeMap, BTreeSet};
    if rows.len() > DIRECTORY_LIMIT {
        return None;
    }
    let mut by = BTreeMap::new();
    for row in rows {
        if !identifier(&row.directory) || by.insert(row.directory.as_str(), row).is_some() {
            return None;
        }
    }
    let anchors = [
        ("TARGETDIR", None, "SourceDir"),
        ("LocalAppDataFolder", Some("TARGETDIR"), "."),
        ("UserPrograms", Some("LocalAppDataFolder"), "Programs"),
        ("FcInstallDir", Some("UserPrograms"), "FutureClassroom"),
    ];
    for (name, parent, default) in anchors {
        let row = by.get(name)?;
        if row.parent.as_deref() != parent || row.default_dir != default {
            return None;
        }
    }
    let mut paths = BTreeMap::new();
    let mut folded = BTreeSet::new();
    paths.insert("FcInstallDir".to_owned(), String::new());
    folded.insert(String::new());
    for row in rows {
        if anchors.iter().any(|a| a.0 == row.directory) {
            continue;
        }
        let mut current = row;
        let mut visited = BTreeSet::new();
        let mut parts = Vec::new();
        while current.directory != "FcInstallDir" {
            if !visited.insert(current.directory.as_str())
                || !segment(&current.default_dir)
                || parts.len() >= 3
            {
                return None;
            }
            parts.push(current.default_dir.as_str());
            current = by.get(current.parent.as_deref()?)?;
        }
        parts.reverse();
        let path = parts.join("/");
        if !folded.insert(path.to_ascii_lowercase()) {
            return None;
        }
        paths.insert(row.directory.clone(), path);
    }
    Some(paths)
}
/// Only the existing closed guard tuple/sequence is recognized. Raw complete
/// tables remain in Captured; this structural match never grants API authority.
pub(super) fn fixed_guard_shape(
    actions: &[CustomActionRow],
    sequence: &[ExecuteSequenceRow],
    binaries: &[String],
) -> bool {
    use std::collections::BTreeSet;
    if actions.len() > CUSTOM_ACTION_LIMIT || sequence.len() > EXECUTE_SEQUENCE_LIMIT {
        return false;
    }
    let mut names = BTreeSet::new();
    if actions
        .iter()
        .any(|r| !identifier(&r.action) || !names.insert(&r.action))
    {
        return false;
    }
    names.clear();
    if sequence
        .iter()
        .any(|r| !identifier(&r.action) || !names.insert(&r.action))
    {
        return false;
    }
    let action = actions
        .iter()
        .find(|r| r.action == "FcControlledInnerGuard");
    let guard = sequence
        .iter()
        .find(|r| r.action == "FcControlledInnerGuard");
    let initialize = sequence.iter().find(|r| r.action == "InstallInitialize");
    matches!(action, Some(r) if r.kind==1 && r.source.as_deref()==Some("FcControlledGuardDll")
        && r.target.as_deref()==Some("FcControlledInnerGuard"))
        && matches!(guard, Some(r) if r.condition.as_deref()==Some("1") && r.sequence==Some(1499))
        && matches!(initialize, Some(r) if r.condition.is_none() && r.sequence==Some(1500))
        && binaries.iter().any(|n| n == "FcControlledGuardDll")
}

#[derive(Debug, Default)]
pub(super) struct CloseState {
    pub(super) view_open: bool,
    pub(super) view_close_return: Option<u32>,
    pub(super) handle_close_return: Option<u32>,
    view_attempted: bool,
    handle_attempted: bool,
}
impl CloseState {
    pub(super) fn view_attempt(&mut self) -> bool {
        if !self.view_open || self.view_attempted || self.handle_attempted {
            return false;
        }
        self.view_attempted = true;
        true
    }
    pub(super) fn view_returned(&mut self, code: u32) {
        self.view_close_return = Some(code);
        if code == 0 {
            self.view_open = false;
        }
    }
    pub(super) fn handle_attempt(&mut self) -> bool {
        if self.view_open || self.handle_attempted {
            return false;
        }
        self.handle_attempted = true;
        true
    }
    pub(super) fn handle_returned(&mut self, code: u32) {
        self.handle_close_return = Some(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combined_native_namespaces_share_one_64_handle_bound() {
        assert!(room(60, 3, 1));
        assert!(!room(60, 4, 1));
        assert!(!room(usize::MAX, 1, 1));
        assert!(room(0, 64, 0));
    }
    #[test]
    fn view_must_return_zero_before_handle_close_and_no_close_can_retry() {
        let mut state = CloseState {
            view_open: true,
            ..Default::default()
        };
        assert!(!state.handle_attempt());
        assert!(state.view_attempt());
        state.view_returned(6);
        assert!(!state.view_attempt());
        assert!(!state.handle_attempt());
        assert_eq!(state.view_close_return, Some(6));
        let mut state = CloseState {
            view_open: true,
            ..Default::default()
        };
        assert!(state.view_attempt());
        state.view_returned(0);
        assert!(state.handle_attempt());
        state.handle_returned(6);
        assert!(!state.handle_attempt());
        assert_eq!(state.handle_close_return, Some(6));
    }
    #[test]
    fn record_close_has_one_attempt_and_success_records_actual_zero() {
        let mut state = CloseState::default();
        assert!(!state.view_attempt());
        assert!(state.handle_attempt());
        state.handle_returned(0);
        assert!(!state.handle_attempt());
        assert_eq!(state.handle_close_return, Some(0));
    }
    #[test]
    fn captured_text_is_bounded_and_has_no_embedded_controls() {
        assert!(text(&"x".repeat(TEXT_LIMIT)));
        assert!(!text(&"x".repeat(TEXT_LIMIT + 1)));
        assert!(!text("value\0suffix"));
        assert!(!text("value\n"));
        assert!(!text(&"😀".repeat(TEXT_LIMIT / 2 + 1)));
        assert_eq!(ROW_LIMIT, 72);
        assert_eq!(BINARY_LIMIT, 32);
        assert_eq!(RECEIPT_LIMIT, 256);
    }
    fn anchors() -> Vec<DirectoryRow> {
        [
            ("TARGETDIR", None, "SourceDir"),
            ("LocalAppDataFolder", Some("TARGETDIR"), "."),
            ("UserPrograms", Some("LocalAppDataFolder"), "Programs"),
            ("FcInstallDir", Some("UserPrograms"), "FutureClassroom"),
        ]
        .into_iter()
        .map(|(directory, parent, default_dir)| DirectoryRow {
            directory: directory.into(),
            parent: parent.map(str::to_owned),
            default_dir: default_dir.into(),
        })
        .collect()
    }
    #[test]
    fn declared_tree_closes_only_to_fixed_per_user_root_and_rejects_aliases() {
        let mut rows = anchors();
        rows.push(DirectoryRow {
            directory: "Web".into(),
            parent: Some("FcInstallDir".into()),
            default_dir: "web".into(),
        });
        rows.push(DirectoryRow {
            directory: "Assets".into(),
            parent: Some("Web".into()),
            default_dir: "assets".into(),
        });
        assert_eq!(
            declared_directory_paths(&rows).unwrap()["Assets"],
            "web/assets"
        );
        for value in [
            "..",
            "web.",
            "web ",
            "x/y",
            "C:evil",
            "short|long",
            "CON.txt",
        ] {
            let mut changed = rows.clone();
            changed[4].default_dir = value.into();
            assert!(declared_directory_paths(&changed).is_none(), "{value}");
        }
        let mut changed = rows.clone();
        changed[3].parent = Some("TARGETDIR".into());
        assert!(declared_directory_paths(&changed).is_none());
        let mut changed = rows.clone();
        changed[4].parent = Some("LocalAppDataFolder".into());
        assert!(declared_directory_paths(&changed).is_none());
        let mut changed = rows.clone();
        changed[4].parent = Some("Assets".into());
        assert!(declared_directory_paths(&changed).is_none());
        let mut changed = rows.clone();
        changed[4].parent = Some("Missing".into());
        assert!(declared_directory_paths(&changed).is_none());
        let mut changed = rows.clone();
        changed.push(DirectoryRow {
            directory: "WebAlias".into(),
            parent: Some("FcInstallDir".into()),
            default_dir: "WEB".into(),
        });
        assert!(declared_directory_paths(&changed).is_none());
        let mut changed = rows.clone();
        changed.push(rows[4].clone());
        assert!(declared_directory_paths(&changed).is_none());
    }
    #[test]
    fn directory_and_row_budgets_do_not_expand_the_live_handle_or_close_ledger() {
        assert_eq!(ROW_LIMIT, 72);
        assert_eq!(RECEIPT_LIMIT, 256);
        let mut rows = anchors();
        for n in 0..DIRECTORY_LIMIT - 4 {
            rows.push(DirectoryRow {
                directory: format!("D{n}"),
                parent: Some("FcInstallDir".into()),
                default_dir: format!("d{n}"),
            });
        }
        assert!(declared_directory_paths(&rows).is_some());
        rows.push(DirectoryRow {
            directory: "Overflow".into(),
            parent: Some("FcInstallDir".into()),
            default_dir: "overflow".into(),
        });
        assert!(declared_directory_paths(&rows).is_none());
        assert!(room(60, 3, 1));
        assert!(!room(60, 4, 1));
        assert!(receipt_room(254, 1, 1));
        assert!(!receipt_room(255, 1, 1));
        assert!(!receipt_room(usize::MAX, 1, 1));
        // The supported a470 shape has 72 file +72 component records plus its
        // six directories/one CA/23 sequence and fixed other rows below256.
        assert!(receipt_room(74 - 28 + 144, 0, 1));
    }
    #[test]
    fn guard_requires_original_type_binary_target_condition_and_sequence_tuple() {
        let actions = vec![CustomActionRow {
            action: "FcControlledInnerGuard".into(),
            kind: 1,
            source: Some("FcControlledGuardDll".into()),
            target: Some("FcControlledInnerGuard".into()),
        }];
        let seq = vec![
            ExecuteSequenceRow {
                action: "FcControlledInnerGuard".into(),
                condition: Some("1".into()),
                sequence: Some(1499),
            },
            ExecuteSequenceRow {
                action: "InstallInitialize".into(),
                condition: None,
                sequence: Some(1500),
            },
        ];
        let binaries = vec!["FcControlledGuardDll".into()];
        assert!(fixed_guard_shape(&actions, &seq, &binaries));
        for kind in [0, 2, 17, 1025] {
            let mut a = actions.clone();
            a[0].kind = kind;
            assert!(!fixed_guard_shape(&a, &seq, &binaries));
        }
        let mut a = actions.clone();
        a[0].source = Some("Other".into());
        assert!(!fixed_guard_shape(&a, &seq, &binaries));
        let mut a = actions.clone();
        a[0].target = Some("Other".into());
        assert!(!fixed_guard_shape(&a, &seq, &binaries));
        for condition in [None, Some("0"), Some("HOST=1"), Some("1 OR Installed")] {
            let mut q = seq.clone();
            q[0].condition = condition.map(str::to_owned);
            assert!(!fixed_guard_shape(&actions, &q, &binaries));
        }
        let mut q = seq.clone();
        q[0].sequence = Some(1498);
        assert!(!fixed_guard_shape(&actions, &q, &binaries));
        assert!(!fixed_guard_shape(&actions, &seq, &[]));
        let mut q = seq.clone();
        q.push(seq[0].clone());
        assert!(!fixed_guard_shape(&actions, &q, &binaries));
    }
}

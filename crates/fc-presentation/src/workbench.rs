//! Platform-independent workbench contracts. Views contain only non-secret text;
//! submitted form ownership transfers directly to an application worker.
use std::{collections::BTreeMap, fmt};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldId(pub u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActionId(pub u16);
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Password,
    Multiline,
    ReadOnly,
    Choice(Vec<Choice>),
    TimeRange,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridRow {
    pub group: String,
    pub label: String,
    pub cells: Vec<String>,
    pub merged: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub columns: Vec<String>,
    pub rows: Vec<GridRow>,
    pub selected: Option<(usize, usize)>,
    pub enabled: bool,
}
impl Grid {
    pub fn validate(&self) -> Result<(), FormError> {
        if !(2..=8).contains(&self.columns.len()) || self.rows.len() > 128 {
            return Err(FormError::TooLarge);
        }
        let text = |s: &str| s.len() <= 1024 && !s.contains('\0');
        if self.columns.iter().any(|s| !text(s))
            || self.rows.iter().any(|r| {
                !text(&r.group)
                    || !text(&r.label)
                    || r.cells.len() != if r.merged { 1 } else { self.columns.len() - 1 }
                    || r.cells.iter().any(|s| !text(s))
            })
        {
            return Err(FormError::InvalidView);
        }
        if self.selected.is_some_and(|(r, c)| {
            r >= self.rows.len() || c >= self.columns.len() || self.rows[r].merged && c > 1
        }) {
            return Err(FormError::InvalidView);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: FieldId,
    pub label: String,
    pub kind: FieldKind,
    /// Password fields must always have an empty initial value.
    pub value: String,
    pub max_utf16: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: ActionId,
    pub label: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub title: String,
    pub description: String,
    pub status: String,
    pub fields: Vec<Field>,
    pub actions: Vec<Action>,
    pub busy: bool,
    pub grid: Option<Grid>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormError {
    TooLarge,
    DuplicateField,
    InvalidView,
}
impl View {
    pub fn validate(&self) -> Result<(), FormError> {
        if self.fields.len() > 16
            || self.actions.len() > 12
            || self.title.len() > 512
            || self.description.len() > 8192
            || self.status.len() > 8192
        {
            return Err(FormError::TooLarge);
        }
        for (i, f) in self.fields.iter().enumerate() {
            if f.id.0 == 0
                || f.label.len() > 512
                || f.max_utf16 == 0
                || f.max_utf16
                    > if f.kind == FieldKind::Multiline {
                        20_000
                    } else {
                        16_384
                    }
                || f.value.len() > 65536
                || f.value.encode_utf16().count() > f.max_utf16
                || f.value.contains('\0')
                || f.label.contains('\0')
                || f.kind == FieldKind::Password && !f.value.is_empty()
                || self.fields[..i].iter().any(|x| x.id == f.id)
            {
                return Err(FormError::InvalidView);
            }
            match &f.kind {
                FieldKind::Choice(choices) => {
                    if choices.len() > 1024
                        || choices.iter().enumerate().any(|(i, c)| {
                            c.value.is_empty()
                                || c.value.len() > 128
                                || c.label.len() > 512
                                || c.value.contains('\0')
                                || c.label.contains('\0')
                                || choices[..i].iter().any(|other| other.value == c.value)
                        })
                        || !f.value.is_empty() && !choices.iter().any(|c| c.value == f.value)
                    {
                        return Err(FormError::InvalidView);
                    }
                }
                FieldKind::TimeRange
                    if !f.value.is_empty()
                        && (f.value.len() != 8
                            || !f.value.bytes().all(|b| b.is_ascii_digit() || b == b'X')) =>
                {
                    return Err(FormError::InvalidView);
                }
                _ => {}
            }
        }
        for (i, a) in self.actions.iter().enumerate() {
            if a.id.0 == 0
                || a.label.len() > 256
                || a.label.contains('\0')
                || self.actions[..i].iter().any(|x| x.id == a.id)
            {
                return Err(FormError::InvalidView);
            }
        }
        if [&self.title, &self.description, &self.status]
            .iter()
            .any(|s| s.contains('\0'))
        {
            return Err(FormError::InvalidView);
        }
        if let Some(grid) = &self.grid {
            grid.validate()?;
        }
        Ok(())
    }
}
/// Never Clone or serializable. Debug is always redacted, including non-secret
/// fields: forms may contain credentials or recovery material in any screen.
#[derive(Default)]
pub struct FormData {
    values: BTreeMap<FieldId, Zeroizing<String>>,
}
impl FormData {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    pub fn insert(&mut self, id: FieldId, value: String) -> Result<(), FormError> {
        let value = Zeroizing::new(value);
        if self.values.contains_key(&id) {
            return Err(FormError::DuplicateField);
        }
        if id.0 == 0 || self.values.len() >= 16 || value.len() > 65536 {
            return Err(FormError::TooLarge);
        }
        self.values.insert(id, value);
        Ok(())
    }
    pub fn get(&self, id: FieldId) -> Option<&str> {
        self.values.get(&id).map(|s| s.as_str())
    }
    /// Trusted worker must immediately move a returned secret into its owning
    /// password/recovery wrapper; never put it back into an ordinary View.
    pub fn take(&mut self, id: FieldId) -> Option<String> {
        self.values.remove(&id).map(|mut s| std::mem::take(&mut *s))
    }
    pub fn allocation_bytes(&self) -> Option<usize> {
        self.values
            .values()
            .try_fold(std::mem::size_of::<Self>(), |n, s| {
                n.checked_add(s.capacity())?.checked_add(128)
            })
    }
}
impl fmt::Debug for FormData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FormData([REDACTED])")
    }
}
#[derive(Debug)]
pub enum Event {
    Submit(ActionId, FormData),
    Input,
    Close,
    GridCell {
        row: usize,
        column: usize,
    },
    LayoutSelect {
        generation: u64,
        scope: fc_domain::layout::LayoutScope,
        id: fc_domain::ids::LayoutNodeId,
    },
    LayoutDrop {
        generation: u64,
        scope: fc_domain::layout::LayoutScope,
        source: fc_domain::ids::LayoutNodeId,
        target: fc_domain::ids::LayoutNodeId,
        before: Option<fc_domain::ids::LayoutNodeId>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_forms_reject_duplicates_and_never_debug_credentials() {
        let mut form = FormData::new();
        form.insert(FieldId(1), "long-private-password".into())
            .unwrap();
        assert_eq!(
            form.insert(FieldId(1), "replacement".into()),
            Err(FormError::DuplicateField)
        );
        assert!(!format!("{form:?}").contains("password"));
        assert_eq!(form.take(FieldId(1)).unwrap(), "long-private-password");
        assert_eq!(form.get(FieldId(1)), None);
    }
    #[test]
    fn multiline_content_accepts_10000_astral_scalars_with_exact_utf16_bound() {
        let text = "😀".repeat(10_000);
        let mut view = View {
            title: "独立文字".into(),
            description: String::new(),
            status: String::new(),
            fields: vec![Field {
                id: FieldId(800),
                label: "正文".into(),
                kind: FieldKind::Multiline,
                value: text.clone(),
                max_utf16: 20_000,
            }],
            actions: Vec::new(),
            busy: false,
            grid: None,
        };
        view.validate().unwrap();
        let mut form = FormData::new();
        form.insert(FieldId(800), text.clone()).unwrap();
        assert_eq!(form.take(FieldId(800)).unwrap(), text);
        view.fields[0].value.push('😀');
        assert_eq!(view.validate(), Err(FormError::InvalidView));
        view.fields[0].value = String::new();
        view.fields[0].max_utf16 = 20_001;
        assert_eq!(view.validate(), Err(FormError::InvalidView));
        view.fields[0].max_utf16 = 20_000;
        view.fields[0].kind = FieldKind::Password;
        assert_eq!(view.validate(), Err(FormError::InvalidView));
    }
    #[test]
    fn view_refuses_secret_initial_values_and_duplicate_native_bindings() {
        let mut v = View {
            title: "工作台".into(),
            description: String::new(),
            status: String::new(),
            fields: vec![Field {
                id: FieldId(1),
                label: "密码".into(),
                kind: FieldKind::Password,
                value: String::new(),
                max_utf16: 256,
            }],
            actions: vec![Action {
                id: ActionId(1),
                label: "登录".into(),
                enabled: true,
            }],
            busy: false,
            grid: None,
        };
        v.validate().unwrap();
        v.fields[0].value = "secret".into();
        assert_eq!(v.validate(), Err(FormError::InvalidView));
        v.fields[0].value.clear();
        v.fields.push(v.fields[0].clone());
        assert_eq!(v.validate(), Err(FormError::InvalidView));
    }
    #[test]
    fn timetable_and_choice_contracts_reject_ambiguous_values_and_malformed_spans() {
        let mut grid = Grid {
            columns: vec!["时间".into(), "周一".into(), "周二".into()],
            rows: vec![GridRow {
                group: "上午".into(),
                label: "第1课".into(),
                cells: vec!["甲科".into(), "乙科".into()],
                merged: false,
            }],
            selected: Some((0, 1)),
            enabled: true,
        };
        grid.validate().unwrap();
        grid.rows[0].merged = true;
        assert_eq!(grid.validate(), Err(FormError::InvalidView));
        grid.rows[0].cells.truncate(1);
        grid.validate().unwrap();
        grid.selected = Some((0, 2));
        assert_eq!(grid.validate(), Err(FormError::InvalidView));
        let mut view = View {
            title: String::new(),
            description: String::new(),
            status: String::new(),
            fields: vec![Field {
                id: FieldId(1),
                label: "科目".into(),
                kind: FieldKind::Choice(vec![Choice {
                    value: "stable-1".into(),
                    label: "甲科".into(),
                }]),
                value: "stable-1".into(),
                max_utf16: 128,
            }],
            actions: vec![],
            busy: false,
            grid: None,
        };
        view.validate().unwrap();
        view.fields[0].kind = FieldKind::Choice(
            (0..1024)
                .map(|index| Choice {
                    value: format!("stable-{index}"),
                    label: format!("候选{index}"),
                })
                .collect(),
        );
        view.validate().unwrap();
        if let FieldKind::Choice(choices) = &mut view.fields[0].kind {
            choices.push(Choice {
                value: "extra".into(),
                label: "超限".into(),
            });
        }
        assert_eq!(view.validate(), Err(FormError::InvalidView));
        view.fields[0].kind = FieldKind::Choice(vec![Choice {
            value: "stable-1".into(),
            label: "甲科".into(),
        }]);
        view.fields[0].value = "任意文字".into();
        assert_eq!(view.validate(), Err(FormError::InvalidView));
        view.fields[0].kind = FieldKind::TimeRange;
        view.fields[0].value = "08XX0900".into();
        view.validate().unwrap();
        view.fields[0].value = "08:20-09:00".into();
        assert_eq!(view.validate(), Err(FormError::InvalidView));
    }
}

/// Logical roles select registered UI-thread native owners; display surfaces
/// and management forms keep separate constructors.
/// These are compile-time roles, never persisted IDs or management capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowRole {
    Display,
    Workbench,
    LayoutEditor,
    Preview,
    Status,
}
impl WindowRole {
    pub const FORMS: [Self; 4] = [
        Self::Workbench,
        Self::LayoutEditor,
        Self::Preview,
        Self::Status,
    ];
}

//! Platform-independent workbench contracts. Views contain only non-secret text;
//! submitted form ownership transfers directly to an application worker.
use std::{collections::BTreeMap, fmt};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldId(pub u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActionId(pub u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Password,
    Multiline,
    ReadOnly,
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
                || f.max_utf16 > 16384
                || f.value.len() > 65536
                || f.value.encode_utf16().count() > f.max_utf16
                || f.value.contains('\0')
                || f.label.contains('\0')
                || f.kind == FieldKind::Password && !f.value.is_empty()
                || self.fields[..i].iter().any(|x| x.id == f.id)
            {
                return Err(FormError::InvalidView);
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
        };
        v.validate().unwrap();
        v.fields[0].value = "secret".into();
        assert_eq!(v.validate(), Err(FormError::InvalidView));
        v.fields[0].value.clear();
        v.fields.push(v.fields[0].clone());
        assert_eq!(v.validate(), Err(FormError::InvalidView));
    }
}

//! Distinct business identifiers; zero is never a valid persisted identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidId;

macro_rules! ids {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);
        impl $name {
            pub const fn new(value: u64) -> Result<Self, InvalidId> {
                if value == 0 { Err(InvalidId) } else { Ok(Self(value)) }
            }
            pub const fn get(self) -> u64 { self.0 }
        }
    )+};
}
ids!(
    PrincipalId,
    SchoolId,
    ClassId,
    SubjectId,
    SemesterId,
    TableId,
    RowId,
    LogicalRowKey,
    HomeworkId,
    OverrideId,
    FolderId,
    CommandId,
    MaterialId
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionOverflow;

macro_rules! versions {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);
        impl $name {
            pub const fn new(value: u64) -> Self { Self(value) }
            pub const fn get(self) -> u64 { self.0 }
            pub fn next(self) -> Result<Self, VersionOverflow> {
                self.0.checked_add(1).map(Self).ok_or(VersionOverflow)
            }
        }
    )+};
}
versions!(Revision, AuthEpoch, SceneGeneration);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifiers_and_versions_do_not_wrap() {
        assert_eq!(PrincipalId::new(0), Err(InvalidId));
        assert_eq!(Revision::new(u64::MAX).next(), Err(VersionOverflow));
        assert_eq!(Revision::default().next().unwrap().get(), 1);
    }
}

//! Canonical logical database byte grammar shared with offline publication.
//! This codec only encodes captured values; it never publishes a predecessor.
use super::{schema, LifecycleError, Result};
pub(super) const DOMAIN: &[u8] = b"FC-SAME-SOURCE-DB-v1\0";
pub(super) fn null(value: bool) -> [u8; 1] {
    [u8::from(value)]
}
pub(super) fn integer(value: i32) -> Result<[u8; 4]> {
    if value == i32::MIN {
        return Err(LifecycleError::Invalid);
    }
    Ok(value.to_le_bytes())
}
pub(super) fn length(value: usize) -> Result<[u8; 4]> {
    Ok(u32::try_from(value)
        .map_err(|_| LifecycleError::Invalid)?
        .to_le_bytes())
}
pub(super) fn table_header(table: &schema::Table, rows: usize) -> Result<Vec<u8>> {
    if rows > super::MAX_ROWS {
        return Err(LifecycleError::Invalid);
    }
    let mut out = Vec::new();
    out.extend(length(table.name.len())?);
    out.extend(table.name.as_bytes());
    for (name, kind) in table.columns {
        out.extend(length(name.len())?);
        out.extend(name.as_bytes());
        out.extend(length(kind.len())?);
        out.extend(kind.as_bytes());
    }
    out.extend(length(rows)?);
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_null_integer_lengths_and_field_order_are_unambiguous() {
        assert_eq!(null(true), [1]);
        assert_eq!(null(false), [0]);
        assert_eq!(integer(-2).unwrap(), [254, 255, 255, 255]);
        assert!(integer(i32::MIN).is_err());
        assert_eq!(length(0).unwrap(), [0; 4]);
        let property = schema::table("Property").unwrap();
        let header = table_header(property, 1).unwrap();
        let mut expected = Vec::new();
        for value in ["Property", "Property", "s72", "Value", "l0"] {
            expected.extend((value.len() as u32).to_le_bytes());
            expected.extend(value.as_bytes());
        }
        expected.extend(1u32.to_le_bytes());
        assert_eq!(header, expected);
        assert!(table_header(property, super::super::MAX_ROWS + 1).is_err());
    }
}

//! Versioned, bounded wire input. Actor, origin and paths never come from JSON.
#![forbid(unsafe_code)]
pub mod calendar_feed;
pub mod http1;
pub mod recovery;
pub mod web;

use serde::de::{self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;

pub const MAX_JSON_BYTES: usize = 65_536;
pub const MAX_DEPTH: u8 = 32;
pub const MAX_CONTAINER_ITEMS: usize = 4096;
pub const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolError {
    TooLarge,
    InvalidJson,
    UnsupportedSchema,
    InvalidCommand,
}
impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "RESOURCE_TOO_LARGE",
            Self::InvalidJson => "INVALID_JSON",
            Self::UnsupportedSchema => "UNSUPPORTED_SCHEMA",
            Self::InvalidCommand => "INVALID_COMMAND",
        })
    }
}
impl std::error::Error for ProtocolError {}

struct Bounded(u8);
impl<'de> DeserializeSeed<'de> for Bounded {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        if self.0 > MAX_DEPTH {
            return Err(de::Error::custom("depth limit"));
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Bounded {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let mut count = 0;
        while seq.next_element_seed(Bounded(self.0 + 1))?.is_some() {
            count += 1;
            if count > MAX_CONTAINER_ITEMS {
                return Err(de::Error::custom("array limit"));
            }
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate key"));
            }
            if keys.len() > MAX_CONTAINER_ITEMS {
                return Err(de::Error::custom("object limit"));
            }
            map.next_value_seed(Bounded(self.0 + 1))?;
        }
        Ok(())
    }
}

/// Preflight preserves duplicate keys at all depths before conversion to a strict DTO.
pub fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ProtocolError> {
    if bytes.len() > MAX_JSON_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let mut d = serde_json::Deserializer::from_slice(bytes);
    Bounded(0)
        .deserialize(&mut d)
        .map_err(|_| ProtocolError::InvalidJson)?;
    d.end().map_err(|_| ProtocolError::InvalidJson)?;
    serde_json::from_slice(bytes).map_err(|_| ProtocolError::InvalidJson)
}

/// V1: SHA256("fc-command-v1\0" + canonical JSON). Object keys are sorted;
/// array order, exact integers and UTF-8 content are preserved.
pub fn fingerprint<T: Serialize>(command: &T) -> Result<[u8; 32], ProtocolError> {
    let value = serde_json::to_value(command).map_err(|_| ProtocolError::InvalidCommand)?;
    let encoded = serde_json::to_vec(&value).map_err(|_| ProtocolError::InvalidCommand)?;
    let mut digest = Sha256::new();
    digest.update(b"fc-command-v1\0");
    digest.update(encoded);
    Ok(digest.finalize().into())
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TextCommandDto {
    pub schema_version: u16,
    pub command_id: String,
    pub expected_revision: u64,
    pub class_id: u64,
    pub subject_id: u64,
    pub text: String,
}
impl TextCommandDto {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ProtocolError::UnsupportedSchema);
        }
        if self.command_id.is_empty()
            || self.command_id.len() > 64
            || !self
                .command_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || self.class_id == 0
            || self.subject_id == 0
            || self.text.len() > 16_384
        {
            return Err(ProtocolError::InvalidCommand);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid() -> &'static [u8] {
        br#"{"schemaVersion":1,"commandId":"demo-1","expectedRevision":0,"classId":1,"subjectId":2,"text":"ok"}"#
    }
    #[test]
    fn strict_dto_and_unicode_duplicate_keys() {
        let dto: TextCommandDto = parse(valid()).unwrap();
        dto.validate().unwrap();
        for data in [
            br#"{"text":"a","te\u0078t":"b"}"#.as_slice(),
            br#"{"nested":{"a":1,"a":2}}"#.as_slice(),
            br#"{"schemaVersion":1,"commandId":"x","expectedRevision":18446744073709551616,"classId":1,"subjectId":1,"text":"x"}"#.as_slice(),
        ] { assert!(parse::<TextCommandDto>(data).is_err()); }
        for extra in [
            "\"role\":\"GLOBAL_ADMIN\"",
            "\"origin\":\"local\"",
            "\"ownerTeacherId\":2",
            "\"actor\":{\"principalId\":1}",
            "\"path\":\"C:\\\\a.txt\"",
        ] {
            let mut injected = String::from_utf8(valid().to_vec()).unwrap();
            injected.pop();
            injected.push(',');
            injected.push_str(extra);
            injected.push('}');
            assert!(
                parse::<TextCommandDto>(injected.as_bytes()).is_err(),
                "{extra}"
            );
        }
    }
    #[test]
    fn bounded_before_conversion_and_no_trailing_values() {
        assert_eq!(
            parse::<serde_json::Value>(&vec![b' '; MAX_JSON_BYTES + 1]),
            Err(ProtocolError::TooLarge)
        );
        let nested = format!("{}0{}", "[".repeat(34), "]".repeat(34));
        assert!(parse::<serde_json::Value>(nested.as_bytes()).is_err());
        assert!(parse::<serde_json::Value>(b"{} {}").is_err());
    }
    #[test]
    fn canonical_order_but_distinct_payloads_and_full_integer_precision() {
        let a: serde_json::Value = parse(br#"{"b":9007199254740993,"a":"x"}"#).unwrap();
        let b: serde_json::Value = parse(br#"{"a":"x","b":9007199254740993}"#).unwrap();
        let c: serde_json::Value = parse(br#"{"a":"x","b":9007199254740992}"#).unwrap();
        assert_eq!(fingerprint(&a).unwrap(), fingerprint(&b).unwrap());
        assert_ne!(fingerprint(&a).unwrap(), fingerprint(&c).unwrap());
    }
    #[test]
    fn canonical_nested_keys_remain_sorted_but_array_order_and_unicode_stay_exact() {
        let a: serde_json::Value = parse(br#"{"z":[{"b":2,"a":1},3],"a":0}"#).unwrap();
        let b: serde_json::Value = parse(br#"{"a":0,"z":[{"a":1,"b":2},3]}"#).unwrap();
        let reordered: serde_json::Value = parse(br#"{"a":0,"z":[3,{"a":1,"b":2}]}"#).unwrap();
        assert_eq!(fingerprint(&a).unwrap(), fingerprint(&b).unwrap());
        assert_ne!(fingerprint(&a).unwrap(), fingerprint(&reordered).unwrap());
        let plain: serde_json::Value = parse(br#"{"text":"x"}"#).unwrap();
        let escaped: serde_json::Value = parse(br#"{"text":"\u0078"}"#).unwrap();
        assert_eq!(fingerprint(&plain).unwrap(), fingerprint(&escaped).unwrap());
    }
}

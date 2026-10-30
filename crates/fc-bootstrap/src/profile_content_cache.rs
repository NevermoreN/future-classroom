//! Process-shared verified text ownership, separate from the worker descriptor
//! budget. Weak accounting survives index eviction and every returned Arc clone.
use fc_domain::{
    ids::Revision,
    layout::MAX_VISIBLE_LEAVES,
    profile_content::{ContentSlotKey, PublishedContentDescriptor, PublishedContentSnapshot},
};
use fc_presentation::scene::ContentTextSource;
use std::sync::{Arc, Mutex, OnceLock, Weak};

pub const TEXT_CACHE_BYTES: usize = 4 * 1024 * 1024;
pub const TEXT_CACHE_RECORDS: usize = 512;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextCacheError {
    InvalidDescriptor,
    HashMismatch,
    Capacity,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextCacheStats {
    pub charged_bytes: usize,
    pub records: usize,
    pub retired_records: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    slot: ContentSlotKey,
    revision: Revision,
    sha256: [u8; 32],
}
struct Record {
    key: Key,
    text: Weak<str>,
    charge: usize,
    indexed: bool,
}
struct State {
    records: Vec<Record>,
    limit: usize,
    record_limit: usize,
}
impl State {
    fn reap(&mut self) {
        // No lookup index or caller can recreate ownership after strong_count=0.
        self.records
            .retain(|record| record.text.strong_count() != 0);
    }
    fn charged(&self) -> usize {
        self.records.iter().map(|record| record.charge).sum()
    }
}
#[derive(Clone)]
pub struct ProfileContentTextCache(Arc<Mutex<State>>);
impl ProfileContentTextCache {
    /// One physical ledger across Backend/application replacement. A newly
    /// created cache handle cannot reset bytes still owned by an old Arc<str>.
    pub fn shared() -> Self {
        static SHARED: OnceLock<Arc<Mutex<State>>> = OnceLock::new();
        Self(
            SHARED
                .get_or_init(|| {
                    Arc::new(Mutex::new(State {
                        records: Vec::new(),
                        limit: TEXT_CACHE_BYTES,
                        record_limit: TEXT_CACHE_RECORDS,
                    }))
                })
                .clone(),
        )
    }
    pub fn frame(&self) -> TextFrame {
        TextFrame {
            cache: self.clone(),
            entries: Vec::new(),
        }
    }
    /// Only drops lookup visibility; live external body ownership remains billed.
    pub fn clear(&self) -> Result<(), TextCacheError> {
        let mut state = self.0.lock().map_err(|_| TextCacheError::Unavailable)?;
        state.reap();
        for record in &mut state.records {
            record.indexed = false;
        }
        Ok(())
    }
    pub fn stats(&self) -> Result<TextCacheStats, TextCacheError> {
        let mut state = self.0.lock().map_err(|_| TextCacheError::Unavailable)?;
        state.reap();
        Ok(TextCacheStats {
            charged_bytes: state.charged(),
            records: state.records.len(),
            retired_records: state
                .records
                .iter()
                .filter(|record| !record.indexed)
                .count(),
        })
    }
    fn insert(&self, key: Key, text: String) -> Result<Arc<str>, TextCacheError> {
        let charge = text_charge(text.len()).ok_or(TextCacheError::Capacity)?;
        let mut state = self.0.lock().map_err(|_| TextCacheError::Unavailable)?;
        state.reap();
        if let Some(record) = state.records.iter_mut().find(|record| record.key == key) {
            if let Some(existing) = record.text.upgrade() {
                // SHA was verified against supplied bytes before this lookup.
                if existing.as_ref() != text {
                    return Err(TextCacheError::HashMismatch);
                }
                record.indexed = true;
                return Ok(existing);
            }
        }
        if state.records.len() >= state.record_limit
            || state
                .charged()
                .checked_add(charge)
                .is_none_or(|n| n > state.limit)
        {
            return Err(TextCacheError::Capacity);
        }
        let text: Arc<str> = Arc::from(text);
        state.records.push(Record {
            key,
            text: Arc::downgrade(&text),
            charge,
            indexed: true,
        });
        Ok(text)
    }
}
impl Default for ProfileContentTextCache {
    fn default() -> Self {
        Self::shared()
    }
}
fn text_charge(length: usize) -> Option<usize> {
    let alignment = std::mem::align_of::<usize>();
    length
        .checked_add(2 * std::mem::size_of::<usize>())?
        .checked_add(alignment - 1)
        .map(|n| n / alignment * alignment)
}
#[derive(Clone)]
struct FrameEntry {
    key: Key,
    text: Arc<str>,
}
/// Owned metadata/Arc capabilities for exactly one hydrated facts set. Arc body
/// allocations belong to the separate 4 MiB ledger, never an unmetered snapshot.
#[derive(Clone)]
pub struct TextFrame {
    cache: ProfileContentTextCache,
    entries: Vec<FrameEntry>,
}
impl std::fmt::Debug for TextFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFrame")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}
impl Default for TextFrame {
    fn default() -> Self {
        ProfileContentTextCache::shared().frame()
    }
}
impl TextFrame {
    pub fn insert_verified(
        &mut self,
        snapshot: &PublishedContentSnapshot,
        text: String,
    ) -> Result<(), TextCacheError> {
        snapshot
            .validate()
            .map_err(|_| TextCacheError::InvalidDescriptor)?;
        let PublishedContentDescriptor::Text {
            characters,
            utf8_bytes,
            sha256,
        } = snapshot.content
        else {
            return Err(TextCacheError::InvalidDescriptor);
        };
        if text.len() != utf8_bytes as usize
            || text.chars().count() != characters as usize
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(TextCacheError::InvalidDescriptor);
        }
        if fc_security::sha256(text.as_bytes()) != sha256 {
            return Err(TextCacheError::HashMismatch);
        }
        let key = Key {
            slot: snapshot.slot,
            revision: snapshot.revision,
            sha256,
        };
        if let Some(existing) = self.entries.iter().find(|entry| entry.key == key) {
            return if existing.text.as_ref() == text {
                Ok(())
            } else {
                Err(TextCacheError::HashMismatch)
            };
        }
        if self.entries.len() >= MAX_VISIBLE_LEAVES
            || self.entries.iter().any(|entry| entry.key.slot == key.slot)
        {
            return Err(TextCacheError::Capacity);
        }
        let text = self.cache.insert(key, text)?;
        self.entries.push(FrameEntry { key, text });
        Ok(())
    }
    pub fn complete_for(&self, snapshots: &[PublishedContentSnapshot]) -> bool {
        if snapshots.len() > MAX_VISIBLE_LEAVES {
            return false;
        }
        let texts: Vec<_> = snapshots
            .iter()
            .filter_map(|snapshot| match snapshot.content {
                PublishedContentDescriptor::Text { sha256, .. } => Some(Key {
                    slot: snapshot.slot,
                    revision: snapshot.revision,
                    sha256,
                }),
                _ => None,
            })
            .collect();
        texts.len() == self.entries.len()
            && texts
                .iter()
                .all(|key| self.entries.iter().any(|entry| entry.key == *key))
    }
    /// Inline descriptors/Arc handles only. Every full UTF8 body is separately
    /// accounted until the final strong Arc dies, including Scene/native owners.
    pub fn allocation_bytes(&self) -> Option<usize> {
        self.entries
            .capacity()
            .checked_mul(std::mem::size_of::<FrameEntry>())
    }
}
impl ContentTextSource for TextFrame {
    fn text(&self, slot: ContentSlotKey, revision: Revision, sha256: [u8; 32]) -> Option<Arc<str>> {
        let key = Key {
            slot,
            revision,
            sha256,
        };
        self.entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.text.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_domain::{ids::LayoutNodeId, layout::LayoutScope};
    fn cache(bytes: usize, records: usize) -> ProfileContentTextCache {
        ProfileContentTextCache(Arc::new(Mutex::new(State {
            records: vec![],
            limit: bytes,
            record_limit: records,
        })))
    }
    fn descriptor(id: u64, text: &str) -> PublishedContentSnapshot {
        PublishedContentSnapshot {
            slot: ContentSlotKey {
                scope: LayoutScope::Public,
                node_id: LayoutNodeId::new(id).unwrap(),
            },
            revision: Revision::new(1),
            view_revision: Revision::new(1),
            current_page: 0,
            content: PublishedContentDescriptor::Text {
                characters: text.chars().count() as u32,
                utf8_bytes: text.len() as u32,
                sha256: fc_security::sha256(text.as_bytes()),
            },
        }
    }
    #[test]
    fn clearing_index_and_dropping_frame_do_not_release_live_external_arc_charge() {
        let cache = cache(text_charge(4).unwrap(), 8);
        let snapshot = descriptor(1, "live");
        let hash = fc_security::sha256(b"live");
        let mut frame = cache.frame();
        frame.insert_verified(&snapshot, "live".into()).unwrap();
        let body = frame.text(snapshot.slot, snapshot.revision, hash).unwrap();
        cache.clear().unwrap();
        drop(frame);
        assert_eq!(cache.stats().unwrap().retired_records, 1);
        let mut newer = cache.frame();
        assert_eq!(
            newer.insert_verified(&descriptor(2, "next"), "next".into()),
            Err(TextCacheError::Capacity)
        );
        drop(body);
        assert_eq!(cache.stats().unwrap().charged_bytes, 0);
        newer
            .insert_verified(&descriptor(2, "next"), "next".into())
            .unwrap();
    }
    #[test]
    fn exact_owner_revision_hash_and_missing_nodes_never_fall_back() {
        let cache = cache(TEXT_CACHE_BYTES, 8);
        let snapshot = descriptor(1, "中文😀");
        let mut frame = cache.frame();
        frame.insert_verified(&snapshot, "中文😀".into()).unwrap();
        let hash = fc_security::sha256("中文😀".as_bytes());
        assert!(frame.text(snapshot.slot, Revision::new(2), hash).is_none());
        assert!(frame
            .text(descriptor(2, "中文😀").slot, snapshot.revision, hash)
            .is_none());
        assert!(frame
            .text(snapshot.slot, snapshot.revision, [0; 32])
            .is_none());
        assert!(frame.complete_for(std::slice::from_ref(&snapshot)));
        assert!(!frame.complete_for(&[]));
        let mut bad = snapshot.clone();
        bad.content = PublishedContentDescriptor::Text {
            characters: 3,
            utf8_bytes: 10,
            sha256: [9; 32],
        };
        assert_eq!(
            cache.frame().insert_verified(&bad, "中文😀".into()),
            Err(TextCacheError::HashMismatch)
        );
    }
    #[test]
    fn retired_metadata_has_a_hard_cap_even_when_bodies_are_tiny() {
        let cache = cache(TEXT_CACHE_BYTES, 2);
        let mut first = cache.frame();
        first
            .insert_verified(&descriptor(1, "a"), "a".into())
            .unwrap();
        cache.clear().unwrap();
        let mut second = cache.frame();
        second
            .insert_verified(&descriptor(2, "b"), "b".into())
            .unwrap();
        cache.clear().unwrap();
        assert_eq!(
            cache
                .frame()
                .insert_verified(&descriptor(3, "c"), "c".into()),
            Err(TextCacheError::Capacity)
        );
        drop(first);
        cache
            .frame()
            .insert_verified(&descriptor(3, "c"), "c".into())
            .unwrap();
        assert!(cache.stats().unwrap().records <= 2);
    }
    #[test]
    fn full_four_mib_budget_counts_retained_bodies_across_many_frames() {
        let cache = cache(TEXT_CACHE_BYTES, TEXT_CACHE_RECORDS);
        let body = "😀".repeat(10_000);
        let mut frames = Vec::new();
        let mut saturated = false;
        for id in 1..=128 {
            let mut frame = cache.frame();
            match frame.insert_verified(&descriptor(id, &body), body.clone()) {
                Ok(()) => frames.push(frame),
                Err(TextCacheError::Capacity) => {
                    saturated = true;
                    break;
                }
                other => panic!("unexpected cache admission result: {other:?}"),
            }
        }
        assert!(saturated);
        let before = cache.stats().unwrap().charged_bytes;
        assert!(
            before <= TEXT_CACHE_BYTES
                && before > TEXT_CACHE_BYTES - text_charge(body.len()).unwrap()
        );
        cache.clear().unwrap();
        assert_eq!(cache.stats().unwrap().charged_bytes, before);
        drop(frames);
        assert_eq!(cache.stats().unwrap().charged_bytes, 0);
    }
    #[test]
    fn a_new_shared_handle_cannot_reset_billing_for_an_old_external_body() {
        let cache = ProfileContentTextCache::shared();
        let snapshot = descriptor(u64::MAX, "held across cache handle replacement");
        let mut frame = cache.frame();
        frame
            .insert_verified(&snapshot, "held across cache handle replacement".into())
            .unwrap();
        let PublishedContentDescriptor::Text { sha256, .. } = snapshot.content else {
            unreachable!()
        };
        let body = frame
            .text(snapshot.slot, snapshot.revision, sha256)
            .unwrap();
        cache.clear().unwrap();
        drop(frame);
        drop(cache);
        let replacement = ProfileContentTextCache::shared();
        assert!(replacement.stats().unwrap().charged_bytes >= text_charge(body.len()).unwrap());
        assert_eq!(body.as_ref(), "held across cache handle replacement");
        drop(body);
        replacement.stats().unwrap();
    }
}

//! Bounded, compile-time registrations. They select implementations, never
//! identities, permissions or native resource ownership. No dynamic plugins.

pub const MAX_REGISTRATIONS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationError {
    Empty,
    Capacity,
    Duplicate,
    Missing,
}

pub struct Registration<K, V> {
    pub key: K,
    pub implementation: V,
}

/// Borrowed static descriptors; validation and lookup allocate no storage.
pub struct Registry<'a, K, V> {
    entries: &'a [Registration<K, V>],
}

impl<'a, K: Copy + Eq, V> Registry<'a, K, V> {
    pub fn new(entries: &'a [Registration<K, V>]) -> Result<Self, RegistrationError> {
        if entries.is_empty() {
            return Err(RegistrationError::Empty);
        }
        if entries.len() > MAX_REGISTRATIONS {
            return Err(RegistrationError::Capacity);
        }
        if entries
            .iter()
            .enumerate()
            .any(|(i, entry)| entries[..i].iter().any(|old| old.key == entry.key))
        {
            return Err(RegistrationError::Duplicate);
        }
        Ok(Self { entries })
    }

    pub fn get(&self, key: K) -> Result<&'a V, RegistrationError> {
        self.entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| &entry.implementation)
            .ok_or(RegistrationError::Missing)
    }

    /// Require the complete closed set, without a fallback implementation.
    pub fn require(&self, keys: &[K]) -> Result<(), RegistrationError> {
        if keys.len() != self.entries.len()
            || keys
                .iter()
                .enumerate()
                .any(|(i, key)| keys[..i].contains(key))
        {
            return Err(RegistrationError::Missing);
        }
        for key in keys {
            self.get(*key)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_ambiguous_registration_cannot_select_another_implementation() {
        let empty: [Registration<u8, fn() -> u8>; 0] = [];
        assert!(matches!(
            Registry::new(&empty),
            Err(RegistrationError::Empty)
        ));
        let duplicate = [
            Registration {
                key: 1,
                implementation: 11,
            },
            Registration {
                key: 1,
                implementation: 22,
            },
        ];
        assert!(matches!(
            Registry::new(&duplicate),
            Err(RegistrationError::Duplicate)
        ));
        let entries = [
            Registration {
                key: 1,
                implementation: 11,
            },
            Registration {
                key: 3,
                implementation: 33,
            },
        ];
        let registry = Registry::new(&entries).unwrap();
        assert_eq!(registry.get(2), Err(RegistrationError::Missing));
        assert_eq!(registry.require(&[1, 2]), Err(RegistrationError::Missing));
        assert_eq!(registry.require(&[1, 1]), Err(RegistrationError::Missing));
        assert_eq!(
            registry.require(&[1, 3, 5]),
            Err(RegistrationError::Missing)
        );
        registry.require(&[3, 1]).unwrap();
        assert_eq!(*registry.get(3).unwrap(), 33);
    }
}

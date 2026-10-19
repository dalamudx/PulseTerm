//! Exact native snapshots: deliberately no Debug or serialization of file bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Identity {
    pub(crate) volume: u64,
    pub(crate) index: [u8; 16],
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FileStamp {
    pub(crate) identity: Identity,
    pub(crate) length: u64,
    pub(crate) written: u64,
    pub(crate) changed: i64,
    pub(crate) created: i64,
}

/// An opaque exact snapshot bound to a native directory, basename and byte budget.
/// No application, credential-use or page authority; never Debug-format its bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct FileSnapshot {
    pub(crate) directory: Identity,
    pub(crate) name: String,
    pub(crate) limit: usize,
    pub(crate) stamp: Option<FileStamp>,
    pub(crate) bytes: Vec<u8>,
}
impl FileSnapshot {
    /// Exact file bytes; missing files have an empty slice but remain a distinct snapshot.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Whether the snapshot was captured from an existing regular file.
    pub fn exists(&self) -> bool {
        self.stamp.is_some()
    }
    pub(crate) fn belongs_to(&self, directory: Identity, name: &str, limit: usize) -> bool {
        self.directory == directory && self.name == name && self.limit == limit
    }
}

pub(crate) fn authorized(action: &impl Fn() -> bool) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> FileSnapshot {
        let id = Identity {
            volume: 5,
            index: [7; 16],
        };
        FileSnapshot {
            directory: id,
            name: "settings.toml".into(),
            limit: 65536,
            stamp: Some(FileStamp {
                identity: id,
                length: 0,
                written: 3,
                changed: 4,
                created: 1,
            }),
            bytes: vec![],
        }
    }
    #[test]
    fn absent_and_empty_are_distinct_cas_states() {
        let a = sample();
        let mut b = a.clone();
        b.stamp = None;
        assert!(a.exists());
        assert!(!b.exists());
        assert!(a.bytes().is_empty());
        assert!(a.bytes() == b.bytes() && a != b);
    }
    #[test]
    fn directory_basename_and_budget_cannot_transfer_a_snapshot() {
        let a = sample();
        let mut directory = a.directory;
        directory.index[15] ^= 1;
        assert!(a.belongs_to(a.directory, "settings.toml", 65536));
        assert!(!a.belongs_to(directory, "settings.toml", 65536));
        assert!(!a.belongs_to(a.directory, "keymap.toml", 65536));
        assert!(!a.belongs_to(a.directory, "settings.toml", 1));
    }
    #[test]
    fn full_identity_and_times_detect_replacements_and_restored_bytes() {
        let a = sample();
        for field in 0..5 {
            let mut b = a.clone();
            let Some(stamp) = b.stamp.as_mut() else {
                panic!("fixture")
            };
            match field {
                0 => stamp.identity.index[15] ^= 1,
                1 => stamp.identity.volume += 1,
                2 => stamp.written += 1,
                3 => stamp.changed += 1,
                _ => stamp.created += 1,
            }
            assert!(a.bytes() == b.bytes() && a != b);
        }
    }
    #[test]
    fn changed_content_or_length_conflicts_even_with_reused_metadata() {
        let a = sample();
        let mut b = a.clone();
        b.bytes = vec![1];
        assert!(a != b);
        let mut b = a.clone();
        if let Some(s) = &mut b.stamp {
            s.length += 1;
        }
        assert!(a != b);
    }
    #[test]
    fn revocation_and_callback_panic_fail_closed() {
        assert!(authorized(&|| true));
        assert!(!authorized(&|| false));
        assert!(!authorized(&|| panic!("fixed authorization fixture")));
    }
}

//! Shared bounds for metadata-only catalogue actions; identities grant no authority.
use std::collections::BTreeSet;
use uuid::Uuid;
pub(crate) fn unique_ids(ids: &[Uuid]) -> Option<BTreeSet<Uuid>> {
    if ids.is_empty() || ids.len() > 256 {
        return None;
    }
    let unique: BTreeSet<_> = ids.iter().copied().collect();
    (unique.len() == ids.len()
        && unique
            .iter()
            .all(|id| id.get_version_num() == 7 && id.get_variant() == uuid::Variant::RFC4122))
    .then_some(unique)
}

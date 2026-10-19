use crate::{DirectoryError::Location, DirectoryResult};

pub(crate) fn sid(bytes: &[u8]) -> DirectoryResult<&[u8]> {
    if bytes.len() < 8 || bytes[0] != 1 || bytes[1] > 15 {
        return Err(Location);
    }
    bytes.get(..8 + usize::from(bytes[1]) * 4).ok_or(Location)
}

fn nt_sid(bytes: &[u8], sub: &[u32]) -> bool {
    bytes.len() == 8 + sub.len() * 4
        && bytes.get(..8) == Some(&[1, sub.len() as u8, 0, 0, 0, 0, 0, 5])
        && bytes[8..]
            .chunks_exact(4)
            .zip(sub)
            .all(|(a, b)| a == b.to_le_bytes())
}

fn privileged(bytes: &[u8]) -> bool {
    nt_sid(bytes, &[18]) || nt_sid(bytes, &[32, 544])
}

fn trusted_owner(bytes: &[u8], user: &[u8]) -> bool {
    bytes == user
        || privileged(bytes)
        || nt_sid(
            bytes,
            &[
                80, 956008885, 3418522649, 1831038044, 1853292631, 2271478464,
            ],
        )
}

/// Validate the bounded native ACL encoding without dereferencing ACE pointers.
pub(crate) fn admit_acl(
    owner: &[u8],
    user: &[u8],
    acl: Option<&[u8]>,
    protected: bool,
    leaf: bool,
) -> DirectoryResult<()> {
    if sid(owner)? != owner
        || sid(user)? != user
        || !trusted_owner(owner, user)
        || (leaf && (owner != user || !protected))
    {
        return Err(Location);
    }
    let acl = acl.ok_or(Location)?;
    if acl.len() < 8 || !matches!(acl[0], 2 | 4) || acl[1] != 0 || acl[6] != 0 || acl[7] != 0 {
        return Err(Location);
    }
    let size = usize::from(u16::from_le_bytes([acl[2], acl[3]]));
    let count = usize::from(u16::from_le_bytes([acl[4], acl[5]]));
    if size < 8 || size > acl.len() || count > (size - 8) / 16 {
        return Err(Location);
    }
    let acl = &acl[..size];
    let mut pos = 8;
    let mut owner_full = false;
    for _ in 0..count {
        let header = acl.get(pos..pos + 4).ok_or(Location)?;
        let len = usize::from(u16::from_le_bytes([header[2], header[3]]));
        if len < 16 || len % 4 != 0 || header[1] & !0x1f != 0 {
            return Err(Location);
        }
        let ace = acl.get(pos..pos + len).ok_or(Location)?;
        let principal = sid(&ace[8..])?;
        if principal.len() + 8 != len || !matches!(header[0], 0 | 1) {
            return Err(Location);
        }
        pos += len;
        if header[1] & 0x08 != 0 {
            // Inherit-only grants still affect future children of a private leaf.
            if leaf && (header[0] != 0 || (principal != user && !privileged(principal))) {
                return Err(Location);
            }
            continue;
        }
        if header[0] == 1 {
            if leaf {
                return Err(Location);
            }
            continue;
        }
        let mask = u32::from_le_bytes([ace[4], ace[5], ace[6], ace[7]]);
        if principal == user && (mask & 0x001f01ff == 0x001f01ff || mask & 0x10000000 != 0) {
            owner_full = true;
        }
        let trusted = if leaf {
            principal == user || privileged(principal)
        } else {
            trusted_owner(principal, user)
        };
        if !trusted {
            // Ancestors may allow reading/traversal and adding new entries, never changing existing ones.
            let safe_parent = 0xa01200af;
            if leaf || mask & !safe_parent != 0 {
                return Err(Location);
            }
        }
    }
    if leaf && !owner_full {
        return Err(Location);
    }
    Ok(())
}

pub(crate) fn directory_attributes(attributes: u32) -> DirectoryResult<()> {
    if attributes & 0x10 == 0 || attributes & 0x400 != 0 {
        return Err(Location);
    }
    Ok(())
}

pub(crate) fn component(name: &[u16]) -> DirectoryResult<()> {
    if name.is_empty()
        || name.len() > 255
        || matches!(name.last(), Some(32 | 46))
        || name
            .iter()
            .any(|&c| c < 32 || [34, 42, 47, 58, 60, 62, 63, 92, 124].contains(&c))
    {
        return Err(Location);
    }
    let stem = name.split(|&c| c == 46).next().ok_or(Location)?;
    if stem.last() == Some(&32) {
        return Err(Location);
    }
    let text = String::from_utf16(stem)
        .map_err(|_| Location)?
        .to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL", "CLOCK$", "CONIN$", "CONOUT$"].contains(&text.as_str()) {
        return Err(Location);
    }
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() == 4
        && (text.starts_with("COM") || text.starts_with("LPT"))
        && matches!(units[3], 49..=57 | 0xb9 | 0xb2 | 0xb3)
    {
        return Err(Location);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

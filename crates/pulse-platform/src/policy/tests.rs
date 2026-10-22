use super::*;

fn user() -> Vec<u8> {
    let mut s = vec![1, 2, 0, 0, 0, 0, 0, 5];
    s.extend(21u32.to_le_bytes());
    s.extend(42u32.to_le_bytes());
    s
}
fn everyone() -> Vec<u8> {
    vec![1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]
}
fn ace(who: &[u8], rights: u32, kind: u8, flags: u8) -> Vec<u8> {
    let mut a = vec![kind, flags];
    a.extend(((8 + who.len()) as u16).to_le_bytes());
    a.extend(rights.to_le_bytes());
    a.extend(who);
    a
}
fn acl(aces: &[Vec<u8>]) -> Vec<u8> {
    let len = 8 + aces.iter().map(Vec::len).sum::<usize>();
    let mut a = vec![2, 0];
    a.extend((len as u16).to_le_bytes());
    a.extend((aces.len() as u16).to_le_bytes());
    a.extend([0, 0]);
    for e in aces {
        a.extend(e);
    }
    a
}
#[test]
fn sqlite_sidecars_only_allow_private_protected_or_fully_inherited_acl() {
    let u = user();
    let direct = acl(&[ace(&u, 0x1f01ff, 0, 0)]);
    let inherited = acl(&[ace(&u, 0x1f01ff, 0, 0x10)]);
    assert!(sqlite_sidecar_acl(&u, &u, Some(&direct), true).is_ok());
    assert_eq!(
        sqlite_sidecar_acl(&u, &u, Some(&direct), false),
        Err(Location)
    );
    assert!(sqlite_sidecar_acl(&u, &u, Some(&inherited), false).is_ok());
    assert_eq!(
        admit_acl(&u, &u, Some(&inherited), false, true),
        Err(Location)
    );
    let broad = acl(&[ace(&u, 0x1f01ff, 0, 0x10), ace(&everyone(), 1, 0, 0x10)]);
    assert_eq!(
        sqlite_sidecar_acl(&u, &u, Some(&broad), false),
        Err(Location)
    );
    assert_eq!(
        sqlite_sidecar_acl(&everyone(), &u, Some(&inherited), false),
        Err(Location)
    );
    assert_eq!(sqlite_sidecar_acl(&u, &u, None, false), Err(Location));
}
#[test]
fn owner_full_protected_is_private() {
    let u = user();
    let a = acl(&[ace(&u, 0x1f01ff, 0, 3)]);
    assert!(admit_acl(&u, &u, Some(&a), true, true).is_ok());
}
#[test]
fn null_empty_unprotected_and_foreign_owner_reject() {
    let u = user();
    let a = acl(&[ace(&u, 0x1f01ff, 0, 0)]);
    for (o, d, p) in [
        (&u, None, true),
        (&u, Some(acl(&[])), true),
        (&u, Some(a.clone()), false),
        (&everyone(), Some(a), true),
    ] {
        assert_eq!(admit_acl(o, &u, d.as_deref(), p, true), Err(Location));
    }
}
#[test]
fn program_acl_distinguishes_read_execution_from_file_and_directory_writes() {
    let u = user();
    let read = acl(&[ace(&everyone(), 0xa01200a9, 0, 0)]);
    assert!(program_acl(&u, &u, Some(&read), false).is_ok());
    for mask in [
        2, 4, 0x10, 0x40, 0x100, 0x10000, 0x40000, 0x80000, 0x40000000, 0x10000000,
    ] {
        for flags in [0, 0x0b] {
            let a = acl(&[ace(&everyone(), mask, 0, flags)]);
            assert_eq!(program_acl(&u, &u, Some(&a), false), Err(Location));
        }
    }
    let a = acl(&[ace(&everyone(), 2, 0, 0)]);
    assert!(admit_acl(&u, &u, Some(&a), false, false).is_ok());
    assert_eq!(
        program_acl(&everyone(), &u, Some(&read), false),
        Err(Location)
    );
    assert_eq!(program_acl(&u, &u, None, false), Err(Location));
}

#[test]
fn program_acl_keeps_trusted_install_and_creator_owner_inheritance() {
    let u = user();
    let creator = [1, 1, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0];
    let a = acl(&[
        ace(&u, 0x1f01ff, 0, 0),
        ace(&creator, 0x1f01ff, 0, 0x0b),
        ace(&everyone(), 0xa01200a9, 0, 0x13),
    ]);
    assert!(program_acl(&u, &u, Some(&a), false).is_ok());
    let a = acl(&[ace(&creator, 0x1f01ff, 0, 0)]);
    assert_eq!(program_acl(&u, &u, Some(&a), false), Err(Location));
    let mut malformed = a;
    malformed[10..12].copy_from_slice(&u16::MAX.to_le_bytes());
    assert_eq!(program_acl(&u, &u, Some(&malformed), false), Err(Location));
}

#[test]
fn broad_leaf_rejects_and_parent_write_is_not_read_only() {
    let u = user();
    for mask in [1, 0x1f01ff, 0x10000000] {
        let a = acl(&[ace(&u, 0x1f01ff, 0, 0), ace(&everyone(), mask, 0, 0)]);
        assert_eq!(admit_acl(&u, &u, Some(&a), true, true), Err(Location));
    }
    for mask in [
        0x40, 0x10000, 0x40000, 0x80000, 0x100, 0x40000000, 0x10000000,
    ] {
        let a = acl(&[ace(&everyone(), mask, 0, 0)]);
        assert_eq!(admit_acl(&u, &u, Some(&a), false, false), Err(Location));
    }
    let a = acl(&[ace(&everyone(), 0x1200af, 0, 0)]);
    assert!(admit_acl(&u, &u, Some(&a), false, false).is_ok());
}
#[test]
fn inherit_only_is_not_an_owner_grant_and_unknown_aces_fail_closed() {
    let u = user();
    for (kind, flags) in [(0, 8), (5, 0), (9, 0), (0, 0x40), (1, 0)] {
        let a = acl(&[ace(&u, 0x1f01ff, kind, flags)]);
        assert_eq!(admit_acl(&u, &u, Some(&a), true, true), Err(Location));
    }
}
#[test]
fn malformed_and_truncated_acl_and_sid_reject() {
    let u = user();
    let original = acl(&[ace(&u, 0x1f01ff, 0, 0)]);
    for len in 0..original.len() {
        assert_eq!(
            admit_acl(&u, &u, Some(&original[..len]), true, true),
            Err(Location)
        );
    }
    for (index, value) in [(0, 3), (2, 0), (4, 255), (10, 0), (17, 16)] {
        let mut a = original.clone();
        a[index] = value;
        assert_eq!(admit_acl(&u, &u, Some(&a), true, true), Err(Location));
    }
}
#[test]
fn reparse_points_and_non_directories_reject() {
    for flags in [0, 0x80, 0x410, 0x400] {
        assert_eq!(directory_attributes(flags), Err(Location));
    }
    assert!(directory_attributes(0x10).is_ok());
}
#[test]
fn device_stream_and_ambiguous_names_reject() {
    for s in [
        "", ".", "..", "name.", "name ", "CON.txt", "com1", "LPT²", "x:y", "a/b", "a\\b", "nul",
        "conin$", "a\0b",
    ] {
        assert_eq!(
            component(&s.encode_utf16().collect::<Vec<_>>()),
            Err(Location),
            "{s:?}"
        );
    }
    for s in ["PulseTerm", "中文", "com10", "settings.toml"] {
        assert!(component(&s.encode_utf16().collect::<Vec<_>>()).is_ok());
    }
}

#[test]
fn trusted_installer_is_only_trusted_for_ancestors() {
    let u = user();
    let mut ti = vec![1, 6, 0, 0, 0, 0, 0, 5];
    for n in [
        80u32, 956008885, 3418522649, 1831038044, 1853292631, 2271478464,
    ] {
        ti.extend(n.to_le_bytes());
    }
    let a = acl(&[ace(&ti, 0x1f01ff, 0, 0)]);
    assert!(admit_acl(&ti, &u, Some(&a), false, false).is_ok());
    let a = acl(&[ace(&u, 0x1f01ff, 0, 0), ace(&ti, 0x1f01ff, 0, 0)]);
    assert_eq!(admit_acl(&u, &u, Some(&a), true, true), Err(Location));
}

#[test]
fn reserved_acl_fields_and_device_space_aliases_reject() {
    let u = user();
    let original = acl(&[ace(&u, 0x1f01ff, 0, 0)]);
    for i in [1, 6, 7] {
        let mut a = original.clone();
        a[i] = 1;
        assert_eq!(admit_acl(&u, &u, Some(&a), true, true), Err(Location));
    }
    for s in ["CON .txt", "COM1 .toml", "LPT² .txt"] {
        assert_eq!(
            component(&s.encode_utf16().collect::<Vec<_>>()),
            Err(Location)
        );
    }
}

#[test]
fn foreign_inherit_only_grants_cannot_expose_future_private_children() {
    let u = user();
    for flags in [0x09, 0x0a, 0x0b] {
        let a = acl(&[ace(&u, 0x1f01ff, 0, 0), ace(&everyone(), 1, 0, flags)]);
        assert_eq!(admit_acl(&u, &u, Some(&a), true, true), Err(Location));
        assert!(admit_acl(&u, &u, Some(&a), false, false).is_ok());
    }
}

#[test]
fn file_aliases_readonly_devices_and_directories_reject() {
    for attributes in [1, 0x10, 0x40, 0x400, 0x410] {
        assert_eq!(file_attributes(attributes, 1), Err(Location));
    }
    for links in [0, 2, u32::MAX] {
        assert_eq!(file_attributes(0x80, links), Err(Location));
    }
    assert!(file_attributes(0x80, 1).is_ok());
    assert!(file_attributes(0x180, 1).is_ok());
}
#[test]
fn file_budget_is_bounded_even_for_empty_or_large_inputs() {
    assert!(file_budget(0, 0));
    assert!(file_budget(65536, 65536));
    assert!(!file_budget(65537, 65536));
    assert!(!file_budget(u64::MAX, 65536));
    assert!(!file_budget(0, usize::MAX));
    assert!(!file_budget(0, 1024 * 1024 + 1));
}

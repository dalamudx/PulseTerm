use super::*;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn fixture() -> std::result::Result<(tempfile::TempDir, Connection), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let db = crate::database::open(&dir.path().join("state.sqlite3"))?;
    Ok((dir, db))
}
#[test]
fn organization_cycle_bounds_cas_revocation_and_reopen() -> TestResult {
    let (dir, db) = fixture()?;
    let owner = Uuid::new_v4();
    let initial = load(&db, owner)?;
    let root = edit(
        &db,
        owner,
        &initial,
        Edit::CreateGroup {
            name: "开发".into(),
            parent: None,
        },
        || true,
    )?;
    let root_id = root.document.groups[0].metadata.id.clone();
    let child = edit(
        &db,
        owner,
        &root,
        Edit::CreateGroup {
            name: "中国".into(),
            parent: Some(root_id.clone()),
        },
        || true,
    )?;
    let child_id = child.document.groups[1].metadata.id.clone();
    assert!(matches!(
        edit(
            &db,
            owner,
            &root,
            Edit::RemoveGroup {
                id: root_id.clone()
            },
            || true
        ),
        Err(StoreError::OrganizationConflict)
    ));
    assert!(
        edit(
            &db,
            owner,
            &child,
            Edit::UpdateGroup {
                id: root_id.clone(),
                name: "循环".into(),
                parent: Some(child_id.clone())
            },
            || true
        )
        .is_err()
    );
    assert!(
        edit(
            &db,
            owner,
            &child,
            Edit::CreateGroup {
                name: "中国".into(),
                parent: Some(root_id.clone())
            },
            || true
        )
        .is_err()
    );
    assert!(
        edit(
            &db,
            owner,
            &child,
            Edit::RemoveGroup { id: root_id },
            || true
        )
        .is_err()
    );
    let calls = std::cell::Cell::new(0);
    assert!(matches!(
        edit(
            &db,
            owner,
            &child,
            Edit::RemoveGroup {
                id: child_id.clone()
            },
            || {
                calls.set(calls.get() + 1);
                calls.get() < 3
            }
        ),
        Err(StoreError::OrganizationAuthorization)
    ));
    assert!(load(&db, owner)?.document == child.document);
    let deleted = edit(
        &db,
        owner,
        &child,
        Edit::RemoveGroup {
            id: child_id.clone(),
        },
        || true,
    )?;
    assert!(deleted.document.groups[1].metadata.deleted);
    assert!(
        edit(
            &db,
            owner,
            &deleted,
            Edit::UpdateGroup {
                id: child_id,
                name: "复活".into(),
                parent: None
            },
            || true
        )
        .is_err()
    );
    drop(db);
    let reopened = crate::database::open(&dir.path().join("state.sqlite3"))?;
    assert!(load(&reopened, owner)?.document == deleted.document);
    Ok(())
}
#[test]
fn organization_assignment_preserves_endpoints_and_rejects_deleted_or_stale_items() -> TestResult {
    let (_dir, db) = fixture()?;
    let owner = Uuid::new_v4();
    let hosts = crate::ssh_hosts::load(&db, owner)?;
    let hosts = crate::ssh_hosts::edit(
        &db,
        owner,
        &hosts,
        crate::ssh_hosts::HostEdit::Create(pulse_core::ssh_endpoint::SshEndpoint::new(
            "主机",
            "fixture.invalid",
            22,
            "user",
        )?),
        || true,
    )?;
    let item = hosts.records()[0].id().to_string();
    let initial = load(&db, owner)?;
    let assigned = edit(
        &db,
        owner,
        &initial,
        Edit::Assign {
            kind: Kind::Host,
            item: item.clone(),
            group: None,
            tags: vec!["生产".into(), "中国".into()],
            favorite: true,
        },
        || true,
    )?;
    assert_eq!(assigned.document.tags.len(), 2);
    assert!(assigned.document.memberships[0].favorite);
    assert_eq!(
        crate::ssh_hosts::load(&db, owner)?.revision(),
        hosts.revision()
    );
    assert!(
        edit(
            &db,
            owner,
            &assigned,
            Edit::Assign {
                kind: Kind::Host,
                item: item.clone(),
                group: None,
                tags: vec!["重复".into(), "重复".into()],
                favorite: false
            },
            || true
        )
        .is_err()
    );
    assert!(load(&db, owner)?.document == assigned.document);
    crate::ssh_hosts::edit(
        &db,
        owner,
        &hosts,
        crate::ssh_hosts::HostEdit::Remove(hosts.records()[0].id()),
        || true,
    )?;
    assert!(matches!(
        edit(
            &db,
            owner,
            &assigned,
            Edit::Assign {
                kind: Kind::Host,
                item,
                group: None,
                tags: vec![],
                favorite: false
            },
            || true
        ),
        Err(StoreError::OrganizationConflict)
    ));
    assert!(load(&db, owner)?.document == assigned.document);
    db.execute(
        "UPDATE app_state SET value='{\"version\":99}' WHERE key=?1",
        [KEY],
    )?;
    assert!(matches!(
        load(&db, owner),
        Err(StoreError::OrganizationRecord)
    ));
    Ok(())
}

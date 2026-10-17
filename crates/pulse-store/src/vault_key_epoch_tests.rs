use super::*;
use zeroize::Zeroizing;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn scoped_credential_record_authenticates_vault_id_role_and_current_epoch_without_key_export()
-> TestResult {
    let scope = confirmed(Uuid::now_v7())?;
    let epoch = scope.available_epoch()?;
    let id = Uuid::now_v7();
    let record = crate::vault_record::CredentialRecord::seal_in_epoch(
        &scope,
        &epoch,
        id,
        "ssh_password",
        b"fixed-scoped-record",
    )?;
    let bytes = record.encode_ciphertext()?;
    let decoded =
        crate::vault_record::CredentialRecord::decode_ciphertext(&bytes, scope.expected_id()?)?;
    assert_eq!(
        decoded
            .open_for_adapter_in_epoch(&scope, &epoch, id, "ssh_password")?
            .expose(),
        b"fixed-scoped-record"
    );
    assert!(
        decoded
            .open_for_adapter_in_epoch(&scope, &epoch, Uuid::now_v7(), "ssh_password")
            .is_err()
    );
    assert!(
        decoded
            .open_for_adapter_in_epoch(&scope, &epoch, id, "private_key")
            .is_err()
    );
    let foreign = confirmed(Uuid::now_v7())?;
    let other = foreign.available_epoch()?;
    assert!(
        decoded
            .open_for_adapter_in_epoch(&foreign, &other, id, "ssh_password")
            .is_err()
    );
    let mut tampered = bytes;
    let last = tampered.last_mut().ok_or("fixture.ciphertext")?;
    *last ^= 1;
    let corrupted =
        crate::vault_record::CredentialRecord::decode_ciphertext(&tampered, scope.expected_id()?)?;
    assert!(
        corrupted
            .open_for_adapter_in_epoch(&scope, &epoch, id, "ssh_password")
            .is_err()
    );
    scope.lock()?;
    assert!(
        decoded
            .open_for_adapter_in_epoch(&scope, &epoch, id, "ssh_password")
            .is_err()
    );
    assert!(
        crate::vault_record::CredentialRecord::seal_in_epoch(
            &scope,
            &epoch,
            id,
            "ssh_password",
            b"fixed"
        )
        .is_err()
    );
    Ok(())
}

fn confirmed(id: Uuid) -> Result<VaultKeyLifecycle, Box<dyn std::error::Error>> {
    let scope = VaultKeyLifecycle::new(id)?;
    guard(&scope.shared)?.key = Some(VaultCipher::from_key_material(Zeroizing::new([7; 32]))?);
    Ok(scope)
}
#[test]
fn exact_available_epoch_encrypts_but_wrong_context_and_foreign_same_id_key_never_alias()
-> TestResult {
    let id = Uuid::now_v7();
    let scope = confirmed(id)?;
    let foreign = confirmed(id)?;
    let epoch = scope.available_epoch()?;
    assert!(epoch.is_current_for(&scope)?);
    assert!(!epoch.is_current_for(&foreign)?);
    let context = SecretContext::new(Uuid::now_v7(), "ssh_password")?;
    let sealed = scope.seal_in_epoch(&epoch, &context, b"fixed-epoch-secret")?;
    let plain = scope.open_for_adapter_in_epoch(&epoch, &context, &sealed)?;
    assert_eq!(plain.expose(), b"fixed-epoch-secret");
    assert!(matches!(
        foreign.seal_in_epoch(&epoch, &context, b"fixed"),
        Err(KeyUseError::Lifecycle(LifecycleError::Stale))
    ));
    let wrong = SecretContext::new(Uuid::now_v7(), "ssh_password")?;
    assert!(
        scope
            .open_for_adapter_in_epoch(&epoch, &wrong, &sealed)
            .is_err()
    );
    Ok(())
}
#[test]
fn lock_and_same_dek_later_epoch_never_upgrade_an_old_operation() -> TestResult {
    let scope = confirmed(Uuid::now_v7())?;
    let old = scope.available_epoch()?;
    scope.lock()?;
    assert!(!old.is_current_for(&scope)?);
    guard(&scope.shared)?.key = Some(VaultCipher::from_key_material(Zeroizing::new([7; 32]))?);
    let fresh = scope.available_epoch()?;
    assert!(fresh.is_current_for(&scope)?);
    assert!(!old.is_current_for(&scope)?);
    let context = SecretContext::new(Uuid::now_v7(), "ssh_password")?;
    assert!(matches!(
        scope.seal_in_epoch(&old, &context, b"fixed"),
        Err(KeyUseError::Lifecycle(LifecycleError::Stale))
    ));
    scope.seal_in_epoch(&fresh, &context, b"fixed")?;
    Ok(())
}
#[test]
fn pending_locked_closed_and_dropped_owner_do_not_publish_available_epoch() -> TestResult {
    let scope = VaultKeyLifecycle::new(Uuid::now_v7())?;
    assert!(scope.available_epoch().is_err());
    let permit = scope.begin()?;
    assert!(scope.available_epoch().is_err());
    drop(permit);
    scope.close()?;
    assert!(scope.available_epoch().is_err());
    let scope = confirmed(Uuid::now_v7())?;
    let epoch = scope.available_epoch()?;
    drop(scope);
    assert!(epoch.owner.upgrade().is_none());
    Ok(())
}
#[test]
fn current_epoch_check_under_crypto_lock_cannot_race_to_a_later_key() -> TestResult {
    let scope = confirmed(Uuid::now_v7())?;
    let epoch = scope.available_epoch()?;
    let held = scope.guard_epoch(&epoch)?;
    assert!(scope.shared.try_lock().is_err());
    drop(held);
    scope.lock()?;
    let context = SecretContext::new(Uuid::now_v7(), "ssh_password")?;
    assert!(scope.seal_in_epoch(&epoch, &context, b"fixed").is_err());
    Ok(())
}
#[test]
fn exhausted_or_poisoned_scope_clears_key_and_cannot_reuse_capability() -> TestResult {
    let scope = confirmed(Uuid::now_v7())?;
    guard(&scope.shared)?.epoch = u64::MAX;
    let epoch = scope.available_epoch()?;
    assert!(matches!(scope.lock(), Err(LifecycleError::Exhausted)));
    assert!(epoch.is_current_for(&scope).is_err());
    let scope = confirmed(Uuid::now_v7())?;
    let epoch = scope.available_epoch()?;
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = scope.shared.lock();
        panic!("fixed.fixture.poison");
    }));
    assert!(failure.is_err());
    assert!(epoch.is_current_for(&scope).is_err());
    let state = scope.shared.lock().unwrap_or_else(|p| p.into_inner());
    assert!(state.closed && state.key.is_none());
    Ok(())
}

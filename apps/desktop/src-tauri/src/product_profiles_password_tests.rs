use super::super::super::vault::Intent;
use super::*;
use pulse_app::{product_document::ProductDocuments, registry::WindowToken};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
};
use pulse_store::{ssh_identities::IdentityEdit, vault_unlock::PasswordInput};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn native_shared_private_store_epoch_ticket_submit_and_navigation_revoke_without_secret_response()
-> TestResult {
    let _serial = super::super::VAULT_TEST_SERIAL.lock().await;
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let window: WindowToken = registry.open_window()?;
    documents.install(window)?;
    documents.started(window)?;
    let nonce = documents.finished(window)?;
    documents.confirm(window, nonce)?;
    let page = documents.current(window, nonce)?;
    let reader = Arc::new(ProfileReader::new());
    let owner = PageVaultOwner::new(registry.clone());
    let bank = PasswordWriteBank::new(&owner);
    let root = tempfile::tempdir()?;
    let directory = root.path().join("state");
    reader
        .vault(
            registry.clone(),
            page.clone(),
            directory.clone(),
            owner.clone(),
            Intent::Create(PasswordInput::new(
                b"fixed-native-password-master".to_vec(),
            )?),
        )
        .await?;
    reader
        .vault(
            registry.clone(),
            page.clone(),
            directory.clone(),
            owner.clone(),
            Intent::Unlock(PasswordInput::new(
                b"fixed-native-password-master".to_vec(),
            )?),
        )
        .await?;
    let store = reader.store.get().ok_or("fixture.store")?.store.clone();
    let snapshot = store.load_ssh_identities(Arc::new(|| true)).await?;
    let vault = store
        .load_vault_ciphertext(Arc::new(|| true))
        .await?
        .vault_id()
        .ok_or("fixture.vault")?;
    let value = SshPasswordIdentity::new(
        "fixed",
        "fixed-user",
        SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, false)?,
    )?;
    let catalog = store
        .edit_ssh_identities(snapshot, IdentityEdit::Create(value), Arc::new(|| true))
        .await?;
    let id = catalog.records().next().ok_or("fixture.identity")?.id();
    let ticket = reader
        .password_prepare(
            registry.clone(),
            page.clone(),
            directory.clone(),
            owner.clone(),
            bank.clone(),
            id,
        )
        .await?;
    assert_eq!(reader.credit.available_permits(), 0);
    let permit = bank.take(&page, ticket)?;
    assert!(
        reader
            .password_submit(
                registry.clone(),
                page.clone(),
                root.path().join("wrong"),
                permit,
                SecretBytes::from_owned(b"fixed-no-wrong-store".to_vec())?
            )
            .await
            .is_err()
    );
    assert_eq!(reader.credit.available_permits(), 1);
    let ticket = reader
        .password_prepare(
            registry.clone(),
            page.clone(),
            directory.clone(),
            owner.clone(),
            bank.clone(),
            id,
        )
        .await?;
    let permit = bank.take(&page, ticket)?;
    reader
        .password_submit(
            registry.clone(),
            page.clone(),
            directory.clone(),
            permit,
            SecretBytes::from_owned(b"fixed-native-stored-password".to_vec())?,
        )
        .await?;
    assert_eq!(reader.credit.available_permits(), 1);
    let filled = store.load_ssh_identities(Arc::new(|| true)).await?;
    // A held SQLite callback makes the supervised submit deterministic without blocking Tokio.
    let ticket = reader
        .password_prepare(
            registry.clone(),
            page.clone(),
            directory.clone(),
            owner.clone(),
            bank.clone(),
            id,
        )
        .await?;
    let permit = bank.take(&page, ticket)?;
    let before = store
        .load_vault_ciphertext(Arc::new(|| true))
        .await?
        .revision();
    let (entered, received) = tokio::sync::oneshot::channel();
    let entered = std::sync::Mutex::new(Some(entered));
    let (release, resume) = std::sync::mpsc::channel();
    let resume = std::sync::Mutex::new(resume);
    let first = std::sync::atomic::AtomicBool::new(true);
    let blocked_store = store.clone();
    let blocking = tokio::spawn(async move {
        blocked_store
            .load_identity_vault_source(Arc::new(move || {
                if first.swap(false, Ordering::SeqCst) {
                    if let Ok(mut s) = entered.lock()
                        && let Some(s) = s.take()
                    {
                        let _ = s.send(());
                    }
                    if !resume.lock().is_ok_and(|r| r.recv().is_ok()) {
                        return false;
                    }
                }
                true
            }))
            .await
    });
    received.await?;
    let mut pending = Box::pin(reader.password_submit(
        registry.clone(),
        page.clone(),
        directory.clone(),
        permit,
        SecretBytes::from_owned(b"fixed-cancelled-native-waiter".to_vec())?,
    ));
    std::future::poll_fn(|cx| match std::future::Future::poll(pending.as_mut(), cx) {
        std::task::Poll::Pending => std::task::Poll::Ready(Ok(())),
        std::task::Poll::Ready(_) => std::task::Poll::Ready(Err("fixture.expected_pending")),
    })
    .await?;
    tokio::task::yield_now().await;
    drop(pending);
    assert_eq!(reader.credit.available_permits(), 0);
    assert!(reader.reserve().is_err());
    release.send(())?;
    blocking.await??;
    let _next = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        reader.credit.clone().acquire_owned(),
    )
    .await??;
    drop(_next);
    assert_eq!(reader.credit.available_permits(), 1);
    assert_eq!(
        store
            .load_vault_ciphertext(Arc::new(|| true))
            .await?
            .revision(),
        before
    );

    assert!(
        filled
            .records()
            .next()
            .ok_or("fixture.identity")?
            .identity()
            .password()
            .has_value()
    );
    let ticket = reader
        .password_prepare(
            registry.clone(),
            page.clone(),
            directory.clone(),
            owner.clone(),
            bank.clone(),
            id,
        )
        .await?;
    let permit = bank.take(&page, ticket)?;
    documents.started(window)?;
    owner.retire_page()?;
    bank.retire()?;
    assert!(
        reader
            .password_submit(
                registry.clone(),
                page,
                directory,
                permit,
                SecretBytes::from_owned(b"fixed-not-retired".to_vec())?
            )
            .await
            .is_err()
    );
    bank.close()?;
    owner.close()?;
    drop(store);
    reader.close();
    reader.shutdown().await?;
    registry.shutdown()?;
    Ok(())
}

#[tokio::test]
#[ignore = "isolated fixed window fixture, explicitly exercised by its controller; never zero-test acceptance"]
async fn identity_password_window_verify() -> TestResult {
    let _serial = super::super::VAULT_TEST_SERIAL.lock().await;
    let directory =
        std::path::PathBuf::from(std::env::var("PULSE_IDENTITY_PASSWORD_VERIFY_DIRECTORY")?);
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or("fixture.repository")?
        .join("target");
    if !directory.is_absolute()
        || !directory.starts_with(target)
        || !directory.components().any(|part| {
            part.as_os_str()
                .to_str()
                .is_some_and(|s| s.starts_with("workspace-identity-password."))
        })
    {
        return Err("fixture.directory".into());
    }
    let (store, worker) = pulse_store::StateStore::open(directory.join("state.db")).await?;
    let source = store.load_identity_vault_source(Arc::new(|| true)).await?;
    let row = source
        .identities()
        .records()
        .find(|row| row.identity().label() == "IDPWONE")
        .ok_or("fixture.identity")?;
    let reference = row.identity().password();
    assert!(reference.has_value());
    let vault = source.vault();
    let id = vault.vault_id().ok_or("fixture.vault")?;
    let scope = pulse_store::vault_lifecycle::VaultKeyLifecycle::new(id)?;
    let request = pulse_store::vault_unlock::begin_validation_unlock(
        &scope,
        PasswordInput::new(b"fixed-window-master-password".to_vec())?,
        vault.envelope().ok_or("fixture.envelope")?.clone(),
    )?;
    let mut ready = request.ready().await?;
    ready.confirm(&scope)?;
    let epoch = scope.available_epoch()?;
    let record = vault
        .credentials()
        .iter()
        .find(|row| row.id() == reference.id())
        .ok_or("fixture.credential")?;
    let plain = record.open_for_adapter_in_epoch(&scope, &epoch, reference.id(), "ssh_password")?;
    if plain.expose() != b"fixed-window-identity-password" {
        return Err("fixture.password_bytes".into());
    }
    drop(plain);
    scope.close()?;
    drop(store);
    worker.join().await?;
    eprintln!("workspace.identity_password_private_byte_reopen_confirmed");
    Ok(())
}

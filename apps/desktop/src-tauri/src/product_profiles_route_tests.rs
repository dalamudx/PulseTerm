use super::*;
use pulse_app::{
    product_document::ProductDocuments, vault_owner::PageVaultOwner,
    vault_password_handoff::PasswordWriteBank,
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_endpoint::SshEndpoint,
    ssh_identity::SshPasswordIdentity,
};
use pulse_store::{
    ssh_hosts::HostEdit, ssh_identities::IdentityEdit, ssh_profiles::ProfileEdit,
    vault_crypto::SecretBytes, vault_unlock::PasswordInput,
};
use std::{future::Future, sync::atomic::AtomicBool, time::Duration};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn original_route_reader_private_store_page_binding_cancelled_waiter_and_retired_sql_keep_credit()
-> TestResult {
    let _serial = super::super::super::VAULT_TEST_SERIAL.lock().await;
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let window = registry.open_window()?;
    documents.install(window)?;
    documents.started(window)?;
    let nonce = documents.finished(window)?;
    documents.confirm(window, nonce)?;
    let page = documents.current(window, nonce)?;
    let reader = Arc::new(ProfileReader::new());
    let owner = PageVaultOwner::new(registry.clone());
    let root = tempfile::tempdir()?;
    let directory = root.path().join("state");
    for intent in [
        super::super::super::super::vault::Intent::Create(PasswordInput::new(
            b"fixed-native-route-master".to_vec(),
        )?),
        super::super::super::super::vault::Intent::Unlock(PasswordInput::new(
            b"fixed-native-route-master".to_vec(),
        )?),
    ] {
        reader
            .vault(
                registry.clone(),
                page.clone(),
                directory.clone(),
                owner.clone(),
                intent,
            )
            .await?;
    }
    let store = reader.store.get().ok_or("fixture.store")?.store.clone();
    let vault = store
        .load_vault_ciphertext(Arc::new(|| true))
        .await?
        .vault_id()
        .ok_or("fixture.vault")?;
    let row = SshPasswordIdentity::new(
        "route-identity",
        "fixed-user",
        SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, false)?,
    )?;
    let identities = store
        .edit_ssh_identities(
            store.load_ssh_identities(Arc::new(|| true)).await?,
            IdentityEdit::Create(row),
            Arc::new(|| true),
        )
        .await?;
    let identity = identities.records().next().ok_or("fixture.identity")?.id();
    let bank = PasswordWriteBank::new(&owner);
    let ticket = reader
        .password_prepare(
            registry.clone(),
            page.clone(),
            directory.clone(),
            owner.clone(),
            bank.clone(),
            identity,
        )
        .await?;
    reader
        .password_submit(
            registry.clone(),
            page.clone(),
            directory.clone(),
            bank.take(&page, ticket)?,
            SecretBytes::from_owned(b"fixed-native-route-secret".to_vec())?,
        )
        .await?;
    let mut hosts = store.load_ssh_hosts().await?;
    for name in ["outer", "leaf"] {
        hosts = store
            .edit_ssh_hosts(
                hosts,
                HostEdit::Create(SshEndpoint::new(
                    name,
                    &format!("{name}.invalid"),
                    22,
                    "fixed-user",
                )?),
                Arc::new(|| true),
            )
            .await?;
    }
    let entrance = |name| {
        hosts
            .records()
            .iter()
            .find(|h| h.endpoint().label() == name)
            .map(|h| h.id())
            .ok_or("fixture.host")
    };
    let outer_entry = entrance("outer")?;
    let leaf_entry = entrance("leaf")?;
    let graph = hosts.catalogue().ok_or("fixture.graph")?;
    let profile = |entry| {
        graph
            .profiles()
            .iter()
            .find(|p| p.endpoint_id() == entry)
            .map(|p| p.metadata().id())
            .ok_or("fixture.profile")
    };
    let outer = profile(outer_entry)?;
    let leaf = profile(leaf_entry)?;
    let snapshot = store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    let snapshot = store
        .edit_ssh_profile_with_jump(
            snapshot,
            ProfileEdit {
                id: outer,
                label: "outer".into(),
                username: "fixed-user".into(),
                identity: Some(identity),
            },
            None,
            Arc::new(|| true),
        )
        .await?;
    store
        .edit_ssh_profile_with_jump(
            snapshot,
            ProfileEdit {
                id: leaf,
                label: "leaf".into(),
                username: "fixed-user".into(),
                identity: Some(identity),
            },
            Some(outer),
            Arc::new(|| true),
        )
        .await?;
    let make = || {
        owner.begin_saved_session_gesture_authorized(
            page.clone(),
            leaf_entry,
            identity,
            Arc::new(|| true),
        )
    };
    let kind = SshRouteIdentity::Password(identity);
    let before = store.load_ssh_hosts().await?;
    let ciphertext = store
        .load_vault_ciphertext(Arc::new(|| true))
        .await?
        .revision();
    let fresh_reader = Arc::new(ProfileReader::new());
    let other_window = registry.open_window()?;
    documents.install(other_window)?;
    documents.started(other_window)?;
    let other_nonce = documents.finished(other_window)?;
    documents.confirm(other_window, other_nonce)?;
    let other_page = documents.current(other_window, other_nonce)?;
    let absent = root.path().join("not-created");
    assert!(
        fresh_reader
            .ssh_route(
                registry.clone(),
                other_page,
                absent.clone(),
                make()?,
                leaf,
                kind
            )
            .await
            .is_err()
    );
    assert!(!absent.exists());
    assert_eq!(fresh_reader.credit.available_permits(), 1);
    fresh_reader.shutdown().await?;
    let (returned, mut route) = reader
        .ssh_route(
            registry.clone(),
            page.clone(),
            directory.clone(),
            make()?,
            leaf,
            kind,
        )
        .await?;
    let (selection, source) = route.take_next_selection()?.into_parts();
    assert!(matches!(
        selection,
        pulse_app::vault_owner::RouteHopGesture::Password(_)
    ));
    assert!(
        source
            .password_source()
            .supports_password_selection(outer_entry, identity)
    );
    assert_eq!(reader.credit.available_permits(), 1);
    assert_eq!(
        returned.load_ssh_hosts().await?.revision(),
        before.revision()
    );
    assert_eq!(
        returned
            .load_vault_ciphertext(Arc::new(|| true))
            .await?
            .revision(),
        ciphertext
    );
    drop(route);
    drop(returned);
    let (entered, received) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let (release, resume) = std::sync::mpsc::channel();
    let resume = Mutex::new(resume);
    let first = AtomicBool::new(true);
    let blocked_store = store.clone();
    let blocking = tokio::spawn(async move {
        blocked_store
            .load_identity_vault_source(Arc::new(move || {
                if first.swap(false, Ordering::SeqCst) {
                    if let Ok(mut signal) = entered.lock()
                        && let Some(signal) = signal.take()
                    {
                        let _sent = signal.send(());
                    }
                    if let Ok(resume) = resume.lock() {
                        let _done = resume.recv_timeout(Duration::from_secs(5));
                    }
                }
                true
            }))
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), received).await??;
    let mut pending = Box::pin(reader.ssh_route(
        registry.clone(),
        page.clone(),
        directory,
        make()?,
        leaf,
        kind,
    ));
    std::future::poll_fn(|cx| match pending.as_mut().poll(cx) {
        std::task::Poll::Pending => std::task::Poll::Ready(()),
        _ => panic!("fixture.supervisor-must-pend"),
    })
    .await;
    assert_eq!(reader.credit.available_permits(), 0);
    drop(pending);
    assert_eq!(reader.credit.available_permits(), 0);
    documents.started(window)?;
    let mut idle = reader.idle.subscribe();
    release.send(())?;
    blocking.await??;
    if !*idle.borrow_and_update() {
        tokio::time::timeout(Duration::from_secs(5), idle.changed()).await??;
    }
    assert_eq!(reader.credit.available_permits(), 1);
    assert_eq!(store.load_ssh_hosts().await?.revision(), before.revision());
    assert_eq!(
        store
            .load_vault_ciphertext(Arc::new(|| true))
            .await?
            .revision(),
        ciphertext
    );
    owner.close()?;
    reader.shutdown().await?;
    drop(store);
    registry.shutdown()?;
    Ok(())
}

#[path = "ssh_route_driver_tests.rs"]
mod driver;

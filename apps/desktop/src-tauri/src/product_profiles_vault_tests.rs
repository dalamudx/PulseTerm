use super::*;
use pulse_app::{product_document::ProductDocuments, registry::WindowToken};
use pulse_store::vault_unlock::PasswordInput;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn waiter_cancel_and_shutdown_retain_credit_until_real_sql_callback_exits() -> TestResult {
    use std::future::Future;
    use std::task::Poll;
    let p = page()?;
    let reader = Arc::new(ProfileReader::new());
    let owner = PageVaultOwner::new(p.registry.clone());
    let root = tempfile::tempdir()?;
    let directory = root.path().join("state");
    reader
        .vault(
            p.registry.clone(),
            p.scope.clone(),
            directory.clone(),
            owner.clone(),
            Intent::Status,
        )
        .await?;
    let store = reader.store.get().ok_or("fixture.store")?.store.clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let entered = Arc::new(Mutex::new(Some(entered_tx)));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release = Mutex::new(release_rx);
    let busy = tokio::spawn(async move {
        store
            .load_vault_ciphertext(Arc::new(move || {
                if let Some(entered) = entered.lock().unwrap_or_else(|p| p.into_inner()).take() {
                    let _sent = entered.send(());
                    let _released = release.lock().unwrap_or_else(|p| p.into_inner()).recv();
                }
                true
            }))
            .await
    });
    entered_rx.await?;
    let mut request = Box::pin(reader.vault(
        p.registry.clone(),
        p.scope.clone(),
        directory,
        owner.clone(),
        Intent::Status,
    ));
    std::future::poll_fn(|cx| match request.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(Ok(())),
        Poll::Ready(_) => Poll::Ready(Err("fixture.expected_pending")),
    })
    .await?;
    tokio::task::yield_now().await;
    drop(request);
    owner.close()?;
    assert_eq!(reader.credit.available_permits(), 0);
    let mut shutdown = Box::pin(reader.shutdown());
    std::future::poll_fn(|cx| match shutdown.as_mut().poll(cx) {
        Poll::Pending => Poll::Ready(Ok(())),
        Poll::Ready(_) => Poll::Ready(Err("fixture.premature_shutdown")),
    })
    .await?;
    assert_eq!(reader.credit.available_permits(), 0);
    release_tx.send(())?;
    let _cancelled_busy = busy.await?;
    shutdown.await?;
    assert_eq!(reader.credit.available_permits(), 1);
    p.registry.shutdown()?;
    Ok(())
}

struct Page {
    registry: SessionRegistry,
    documents: ProductDocuments,
    owner: WindowToken,
    scope: Arc<PageLaunchScope>,
}
fn page() -> std::result::Result<Page, Box<dyn std::error::Error>> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window()?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let scope = documents.current(owner, nonce)?;
    Ok(Page {
        registry,
        documents,
        owner,
        scope,
    })
}
fn state(status: Status) -> std::result::Result<serde_json::Value, serde_json::Error> {
    serde_json::to_value(status)
}
#[tokio::test]
async fn actual_shared_store_source_creation_auth_lock_and_namespace_without_secret_response()
-> TestResult {
    let _serial = super::super::VAULT_TEST_SERIAL.lock().await;
    let p = page()?;
    let reader = Arc::new(ProfileReader::new());
    let owner = PageVaultOwner::new(p.registry.clone());
    let root = tempfile::tempdir()?;
    let directory = root.path().join("state");
    let run = |intent| {
        reader.vault(
            p.registry.clone(),
            p.scope.clone(),
            directory.clone(),
            owner.clone(),
            intent,
        )
    };
    assert_eq!(
        state(run(Intent::Status).await?)?,
        serde_json::json!({"version":1,"state":"missing"})
    );
    assert_eq!(
        state(
            run(Intent::Create(PasswordInput::new(
                b"fixed-service-validation-password".to_vec()
            )?))
            .await?
        )?["state"],
        "locked"
    );
    assert!(
        run(Intent::Create(PasswordInput::new(
            b"fixed-service-validation-password".to_vec()
        )?))
        .await
        .is_err()
    );
    assert!(
        run(Intent::Unlock(PasswordInput::new(
            b"wrong-fixed-service-password".to_vec()
        )?))
        .await
        .is_err()
    );
    assert_eq!(state(run(Intent::Status).await?)?["state"], "locked");
    assert_eq!(
        state(
            run(Intent::Unlock(PasswordInput::new(
                b"fixed-service-validation-password".to_vec()
            )?))
            .await?
        )?["state"],
        "available"
    );
    assert_eq!(state(run(Intent::Lock).await?)?["state"], "locked");
    let bank = pulse_app::vault_handoff::VaultHandoffBank::new(&owner);
    for cancelled in [true, false] {
        let token = bank.prepare(
            p.scope.clone(),
            pulse_app::vault_handoff::HandoffKind::Unlock,
        )?;
        let ticket = bank.begin(
            token,
            &p.scope,
            pulse_app::vault_handoff::HandoffKind::Unlock,
        )?;
        let ready = reader
            .vault_ticket(
                p.registry.clone(),
                p.scope.clone(),
                directory.clone(),
                owner.clone(),
                Intent::Unlock(PasswordInput::new(
                    b"fixed-service-validation-password".to_vec(),
                )?),
                ticket,
            )
            .await?;
        assert_eq!(state(ready)?["state"], "pending");
        assert_eq!(reader.credit.available_permits(), 0);
        if cancelled {
            bank.cancel(token, &p.scope)?;
        } else {
            bank.confirm(token, &p.scope)?;
        }
        assert_eq!(reader.credit.available_permits(), 1);
        assert_eq!(
            state(run(Intent::Status).await?)?["state"],
            if cancelled { "locked" } else { "available" }
        );
        assert!(bank.confirm(token, &p.scope).is_err());
    }
    bank.close()?;
    owner.close()?;
    assert!(
        reader
            .identities(p.registry.clone(), p.scope.clone(), directory.clone())
            .await?
            .vault_id()
            .is_some()
    );
    reader.shutdown().await?;
    p.registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn queued_page_retirement_and_directory_switch_never_publish_or_initialize_default()
-> TestResult {
    let p = page()?;
    let reader = Arc::new(ProfileReader::new());
    let owner = PageVaultOwner::new(p.registry.clone());
    let root = tempfile::tempdir()?;
    let absent = root.path().join("absent");
    let pending = reader.vault(
        p.registry.clone(),
        p.scope.clone(),
        absent.clone(),
        owner.clone(),
        Intent::Status,
    );
    p.documents.started(p.owner)?;
    owner.retire_page()?;
    assert!(pending.await.is_err());
    assert!(!absent.exists());
    let nonce = p.documents.finished(p.owner)?;
    p.documents.confirm(p.owner, nonce)?;
    let current = p.documents.current(p.owner, nonce)?;
    let first = root.path().join("first");
    assert_eq!(
        state(
            reader
                .vault(
                    p.registry.clone(),
                    current.clone(),
                    first,
                    owner.clone(),
                    Intent::Status
                )
                .await?
        )?["state"],
        "missing"
    );
    let second = root.path().join("second");
    assert!(
        reader
            .vault(
                p.registry.clone(),
                current,
                second.clone(),
                owner,
                Intent::Status
            )
            .await
            .is_err()
    );
    assert!(!second.exists());
    reader.shutdown().await?;
    p.registry.shutdown()?;
    Ok(())
}

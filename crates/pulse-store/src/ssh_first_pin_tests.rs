use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_pin_native_cancel_retains_credit_until_actual_sql_exit_and_rolls_back() -> TestResult
{
    let dir = private()?;
    let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
    let hosts = store.load_ssh_hosts().await?;
    let hosts = store
        .edit_ssh_hosts(
            hosts,
            HostEdit::Create(SshEndpoint::new("fixed", "fixed.example", 22, "user")?),
            Arc::new(|| true),
        )
        .await?;
    let host = hosts.records()[0].id();
    let source = store.load_ssh_target_source(Arc::new(|| true)).await?;
    let (sent, entered) = tokio::sync::oneshot::channel();
    let sent = std::sync::Mutex::new(Some(sent));
    let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let blocked = gate.clone();
    let credit = Arc::new(());
    let weak = Arc::downgrade(&credit);
    let authorize: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        std::hint::black_box(&credit);
        if let Some(sent) = sent.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _entered = sent.send(());
            let (mutex, signal) = &*blocked;
            let mut open = mutex.lock().unwrap_or_else(|p| p.into_inner());
            while !*open {
                open = signal.wait(open).unwrap_or_else(|p| p.into_inner());
            }
        }
        true
    });
    let owned = store.clone();
    let task = tokio::spawn(async move {
        owned
            .approve_first_ssh_pin(source, host, pin("fixed.example")?, authorize)
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), entered).await??;
    task.abort();
    assert!(task.await.is_err());
    assert!(weak.upgrade().is_some());
    {
        *gate.0.lock().unwrap_or_else(|p| p.into_inner()) = true;
    }
    gate.1.notify_all();
    assert_eq!(store.load_ssh_trust().await?.revision(), 0);
    assert!(weak.upgrade().is_none());
    worker.shutdown().await?;
    Ok(())
}

fn pin(host: &str) -> crate::Result<TrustRecord> {
    TrustRecord::new(TrustTarget::new(host, 22)?, "ssh-ed25519 AAAA".into())
}
#[test]
fn first_pin_full_source_target_final_revoke_and_known_pin_never_overwrites() -> TestResult {
    let dir = private()?;
    let c = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let empty = load(&c, owner)?;
    let hosts = ssh_hosts::edit(
        &c,
        owner,
        &empty.hosts,
        HostEdit::Create(SshEndpoint::new("fixed", "fixed.example", 22, "user")?),
        || true,
    )?;
    let host = hosts.records()[0].id();
    let source = load(&c, owner)?;
    assert!(matches!(
        approve_first(
            &c,
            Uuid::new_v4(),
            &source,
            host,
            pin("fixed.example")?,
            || true
        ),
        Err(StoreError::IdentityConflict)
    ));
    assert!(matches!(
        approve_first(&c, owner, &source, host, pin("wrong.example")?, || true),
        Err(StoreError::HostTrustConflict)
    ));
    assert!(matches!(
        approve_first(
            &c,
            owner,
            &source,
            Uuid::new_v4(),
            pin("fixed.example")?,
            || true
        ),
        Err(StoreError::HostConflict)
    ));
    assert!(matches!(
        approve_first(&c, owner, &source, host, pin("fixed.example")?, || false),
        Err(StoreError::IdentityAuthorization)
    ));
    let calls = AtomicUsize::new(0);
    assert!(matches!(
        approve_first(&c, owner, &source, host, pin("fixed.example")?, || calls
            .fetch_add(1, Ordering::SeqCst)
            == 0),
        Err(StoreError::IdentityAuthorization)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(load(&c, owner)?.trust.revision(), 0);
    let approved = approve_first(&c, owner, &source, host, pin("fixed.example")?, || true)?;
    assert_eq!(approved.trust.revision(), 1);
    assert_eq!(approved.trust.records().len(), 1);
    assert!(matches!(
        approve_first(&c, owner, &source, host, pin("fixed.example")?, || true),
        Err(StoreError::IdentityConflict)
    ));
    for record in [
        pin("fixed.example")?,
        TrustRecord::new(
            TrustTarget::new("fixed.example", 22)?,
            "ssh-rsa AAAA".into(),
        )?,
    ] {
        assert!(matches!(
            approve_first(&c, owner, &approved, host, record, || true),
            Err(StoreError::HostTrustConflict)
        ));
    }
    assert_eq!(load(&c, owner)?.trust.revision(), 1);
    // Same-revision endpoint tampering still invalidates the entire old prompt source.
    let hosts = ssh_hosts::edit(
        &c,
        owner,
        &approved.hosts,
        HostEdit::Create(SshEndpoint::new("second", "second.example", 22, "user")?),
        || true,
    )?;
    let source = load(&c, owner)?;
    let second = hosts.records()[1].id();
    for key in [ssh_hosts::KEY, crate::host_catalogue::KEY] {
        c.execute(
            "UPDATE app_state SET value=replace(value,'second','changed') WHERE key=?1",
            [key],
        )?;
    }
    assert!(matches!(
        approve_first(&c, owner, &source, second, pin("second.example")?, || true),
        Err(StoreError::IdentityConflict)
    ));
    assert_eq!(load(&c, owner)?.trust.records().len(), 1);
    Ok(())
}

#[tokio::test]
async fn first_pin_typed_worker_reopen_and_foreign_instance_reject_preserve_shared_store()
-> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    let hosts = store.load_ssh_hosts().await?;
    let hosts = store
        .edit_ssh_hosts(
            hosts,
            HostEdit::Create(SshEndpoint::new("fixed", "fixed.example", 22, "user")?),
            Arc::new(|| true),
        )
        .await?;
    let host = hosts.records()[0].id();
    let source = store.load_ssh_target_source(Arc::new(|| true)).await?;
    let (foreign, other_task) = StateStore::open(path.clone()).await?;
    assert!(matches!(
        foreign
            .approve_first_ssh_pin(
                source.clone(),
                host,
                pin("fixed.example")?,
                Arc::new(|| true)
            )
            .await,
        Err(StoreError::IdentityConflict)
    ));
    assert!(matches!(
        store
            .approve_first_ssh_pin(
                source.clone(),
                host,
                pin("fixed.example")?,
                Arc::new(|| false)
            )
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    let approved = store
        .approve_first_ssh_pin(source, host, pin("fixed.example")?, Arc::new(|| true))
        .await?;
    assert_eq!(approved.trust().revision(), 1);
    assert_eq!(
        foreign
            .load_ssh_target_source(Arc::new(|| true))
            .await?
            .trust()
            .revision(),
        1
    );
    other_task.shutdown().await?;
    task.shutdown().await?;
    let (reopened, task) = StateStore::open(path.clone()).await?;
    let source = reopened.load_ssh_target_source(Arc::new(|| true)).await?;
    assert_eq!(source.trust().records().len(), 1);
    assert!(matches!(
        reopened
            .approve_first_ssh_pin(source, host, pin("fixed.example")?, Arc::new(|| true))
            .await,
        Err(StoreError::HostTrustConflict)
    ));
    task.shutdown().await?;
    Ok(())
}

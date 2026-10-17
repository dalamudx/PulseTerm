use super::*;
use crate::managed_store::{FirstPinApproval, FirstPinError};
use pulse_core::ssh_endpoint::SshEndpoint;
use pulse_store::{StateStore, ssh_hosts::HostEdit};

#[tokio::test]
async fn actual_first_use_cancel_guard_foreign_source_drift_known_key_and_one_commit_auth_zero()
-> TestResult {
    let secret = key()?;
    let good = secret.public_key().clone();
    let cfg = Arc::new(server::Config {
        keys: vec![secret],
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let auth = Arc::new(AtomicUsize::new(0));
    let mut spy = Spy {
        auth: auth.clone(),
        active: Arc::new(AtomicUsize::new(0)),
        child: false,
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let running = spy.run_on_socket(cfg, &listener);
        let _sent = tx.send(running.handle());
        running.await
    });
    let mut listener = ListenerGuard {
        stop: rx.await?,
        task: Some(task),
    };
    let dir = tempfile::tempdir()?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let hosts = store.load_ssh_hosts().await?;
    let hosts = store
        .edit_ssh_hosts(
            hosts,
            HostEdit::Create(SshEndpoint::new(
                "fixed",
                "127.0.0.1",
                address.port(),
                "user",
            )?),
            Arc::new(|| true),
        )
        .await?;
    let host = hosts.records()[0].id();
    let current = Arc::new(AtomicBool::new(true));
    let live = current.clone();
    let scope: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || live.load(Ordering::Acquire));
    let config = Arc::new(client::Config::default());
    for mode in 0..4 {
        let source = store.load_ssh_target_source(scope.clone()).await?;
        let prompt = FirstPinApproval::observe(
            source,
            host,
            Some(vec![]),
            config.clone(),
            scope.clone(),
            tokio::net::TcpStream::connect(address).await?,
        )
        .await?;
        assert_eq!(prompt.candidate().public_key(), &good);
        assert_eq!(auth.load(Ordering::SeqCst), 0);
        match mode {
            0 => assert!(matches!(
                prompt.confirm(&store, false).await,
                Err(FirstPinError::Denied)
            )),
            1 => {
                current.store(false, Ordering::Release);
                assert!(matches!(
                    prompt.confirm(&store, true).await,
                    Err(FirstPinError::Denied)
                ));
                current.store(true, Ordering::Release);
            }
            2 => {
                let (foreign, task) = StateStore::open(path.clone()).await?;
                assert!(matches!(
                    prompt.confirm(&foreign, true).await,
                    Err(FirstPinError::Source)
                ));
                task.shutdown().await?;
            }
            _ => {
                let hosts = store.load_ssh_hosts().await?;
                store
                    .edit_ssh_hosts(
                        hosts,
                        HostEdit::Update(
                            host,
                            SshEndpoint::new("changed", "127.0.0.1", address.port(), "user")?,
                        ),
                        scope.clone(),
                    )
                    .await?;
                assert!(matches!(
                    prompt.confirm(&store, true).await,
                    Err(FirstPinError::Source)
                ));
            }
        }
        assert_eq!(store.load_ssh_trust().await?.revision(), 0);
    }
    let source = store.load_ssh_target_source(scope.clone()).await?;
    let prompt = FirstPinApproval::observe(
        source,
        host,
        Some(vec![]),
        config.clone(),
        scope.clone(),
        tokio::net::TcpStream::connect(address).await?,
    )
    .await?;
    // An additional native SQL reservation can only restrict the original current prompt.
    for panic_guard in [false, true] {
        let source = store.load_ssh_target_source(scope.clone()).await?;
        let restricted = FirstPinApproval::observe(
            source,
            host,
            Some(vec![]),
            config.clone(),
            scope.clone(),
            tokio::net::TcpStream::connect(address).await?,
        )
        .await?;
        let guard: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
            if panic_guard {
                panic!("fixed write guard panic");
            }
            false
        });
        assert!(matches!(
            restricted.confirm_with_guard(&store, true, guard).await,
            Err(FirstPinError::Source)
        ));
        assert_eq!(store.load_ssh_trust().await?.revision(), 0);
    }
    assert_eq!(prompt.confirm(&store, true).await?, 1);
    // Even matching known keys never create a first-use approval prompt.
    let source = store.load_ssh_target_source(scope.clone()).await?;
    assert!(matches!(
        FirstPinApproval::observe(
            source,
            host,
            Some(vec![]),
            config.clone(),
            scope.clone(),
            tokio::net::TcpStream::connect(address).await?
        )
        .await,
        Err(FirstPinError::Known)
    ));
    let target = HostTarget::new("127.0.0.1", address.port())?;
    let wrong = key()?.public_key().clone();
    let trust = store.load_ssh_trust().await?;
    store
        .edit_ssh_trust(trust, crate::managed_store::pin_edit(&target, &wrong)?)
        .await?;
    let source = store.load_ssh_target_source(scope.clone()).await?;
    assert!(matches!(
        FirstPinApproval::observe(
            source,
            host,
            Some(vec![]),
            config,
            scope,
            tokio::net::TcpStream::connect(address).await?
        )
        .await,
        Err(FirstPinError::Known)
    ));
    assert_eq!(store.load_ssh_trust().await?.revision(), 2);
    assert_eq!(auth.load(Ordering::SeqCst), 0);
    worker.shutdown().await?;
    listener.stop.shutdown("fixed first pin test end".into());
    if let Some(task) = listener.task.take() {
        tokio::time::timeout(std::time::Duration::from_secs(2), task).await???;
    }
    Ok(())
}

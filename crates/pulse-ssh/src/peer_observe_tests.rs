use super::*;
use crate::peer::{OfferedKeyStatus, observe_host_key};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Admission {
    released: Arc<tokio::sync::Notify>,
}
impl Drop for Admission {
    fn drop(&mut self) {
        self.released.notify_one();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_spawned_handshake_cancel_and_original_timeout_close_io_without_candidate()
-> TestResult {
    for cancel in [true, false] {
        let (stream, mut remote) = tokio::io::duplex(65536);
        let released = Arc::new(tokio::sync::Notify::new());
        let admission = Arc::new(Admission {
            released: released.clone(),
        });
        let weak = Arc::downgrade(&admission);
        let task = tokio::spawn(observe_host_key(
            HostTarget::new("fixed.invalid", 22)?,
            Some(vec![]),
            Some(vec![]),
            Arc::new(client::Config::default()),
            admission,
            stream,
        ));
        remote.write_all(b"SSH-2.0-fixed-observation\r\n").await?;
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut byte = [0];
            for _ in 0..256 {
                remote.read_exact(&mut byte).await?;
                if byte[0] == b'\n' {
                    break;
                }
            }
            let length = remote.read_u32().await? as usize;
            if !(1..=16384).contains(&length) {
                return Err(std::io::Error::other("fixed.observation.packet"));
            }
            remote.read_exact(&mut vec![0; length]).await?;
            Ok::<(), std::io::Error>(())
        })
        .await??;
        // The KEX packet proves the detached SDK session now owns the stream.
        assert!(weak.upgrade().is_some());
        if cancel {
            task.abort();
            assert!(task.await.is_err());
        } else {
            assert!(matches!(
                tokio::time::timeout(std::time::Duration::from_secs(7), task).await??,
                Err(PeerError::Transport)
            ));
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), released.notified()).await?;
        assert!(weak.upgrade().is_none());
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), remote.read(&mut byte))
                .await??,
            0
        );
    }
    Ok(())
}

#[tokio::test]
async fn unavailable_sources_drop_owned_io_before_reservation_without_ssh_bytes() -> TestResult {
    let target = HostTarget::new("fixed.invalid", 22)?;
    for own in [None, Some(vec![])] {
        let (stream, mut remote) = tokio::io::duplex(64);
        let admission = Arc::new(());
        let error = observe_host_key(
            target.clone(),
            own,
            None,
            Arc::new(client::Config::default()),
            admission.clone(),
            stream,
        )
        .await;
        assert!(matches!(
            error,
            Err(PeerError::HostKey(HostKeyError::StoreUnavailable))
        ));
        assert_eq!(Arc::strong_count(&admission), 1);
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), remote.read(&mut byte))
                .await??,
            0
        );
    }
    Ok(())
}

#[tokio::test]
async fn actual_unknown_changed_and_pinned_offers_are_display_only_auth_zero_and_io_drained()
-> TestResult {
    let secret = key()?;
    let good = secret.public_key().clone();
    let bad = key()?.public_key().clone();
    let cfg = Arc::new(server::Config {
        keys: vec![secret],
        auth_rejection_time: std::time::Duration::ZERO,
        auth_rejection_time_initial: Some(std::time::Duration::ZERO),
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let auth = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let mut spy = Spy {
        auth: auth.clone(),
        active,
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
    let target = HostTarget::new("127.0.0.1", address.port())?;
    let config = Arc::new(client::Config::default());
    for (own, system, status) in [
        (vec![], Some(vec![]), OfferedKeyStatus::Unknown),
        (
            vec![HostPin::new(target.clone(), bad)],
            Some(vec![HostPin::new(target.clone(), good.clone())]),
            OfferedKeyStatus::Changed,
        ),
        (
            vec![HostPin::new(target.clone(), good.clone())],
            None,
            OfferedKeyStatus::AlreadyPinned,
        ),
    ] {
        let admission = Arc::new(());
        let candidate = observe_host_key(
            target.clone(),
            Some(own),
            system,
            config.clone(),
            admission.clone(),
            tokio::net::TcpStream::connect(address).await?,
        )
        .await?;
        assert_eq!(candidate.target(), &target);
        assert_eq!(candidate.public_key(), &good);
        assert_eq!(candidate.status(), status);
        assert_eq!(auth.load(Ordering::SeqCst), 0);
        assert_eq!(Arc::strong_count(&admission), 1);
        drop(candidate);
    }
    listener.stop.shutdown("fixed observation test end".into());
    if let Some(task) = listener.task.take() {
        tokio::time::timeout(std::time::Duration::from_secs(2), task).await???;
    }
    Ok(())
}

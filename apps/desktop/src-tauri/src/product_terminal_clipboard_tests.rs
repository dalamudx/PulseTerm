use super::*;
#[tokio::test]
async fn clipboard_permission_requires_commit_input_same_view_one_shot_and_page() {
    let f = Fixture::new();
    let p = f.prepare().await;
    let key = ticket(&p.ticket).unwrap();
    assert!(f.bank.prepare_clipboard(f.nonce, key).is_err());
    f.finish().await;
    let f = Fixture::new();
    let key = input_ready(&f).await;
    assert!(f.bank.prepare_clipboard(Uuid::new_v4(), key).is_err());
    let id = f.bank.prepare_clipboard(f.nonce, key).unwrap();
    assert!(f.bank.prepare_clipboard(f.nonce, key).is_err());
    f.bank.cancel_clipboard(f.nonce, Uuid::new_v4()).unwrap();
    assert!(f.bank.begin_clipboard(Uuid::new_v4(), id).is_err());
    let job = f.bank.begin_clipboard(f.nonce, id).unwrap();
    assert!(job.current(&f.bank).is_ok());
    assert!(f.bank.begin_clipboard(f.nonce, id).is_err());
    drop(job);
    assert!(f.bank.begin_clipboard(f.nonce, id).is_err());
    f.bank.cancel_clipboard(f.nonce, id).unwrap();
    let id = f.bank.prepare_clipboard(f.nonce, key).unwrap();
    f.bank.cancel_clipboard(f.nonce, id).unwrap();
    assert!(f.bank.begin_clipboard(f.nonce, id).is_err());
    let id = f.bank.prepare_clipboard(f.nonce, key).unwrap();
    let job = f.bank.begin_clipboard(f.nonce, id).unwrap();
    let _new = f.registry.activate_launch_page(f.owner).unwrap();
    assert!(job.current(&f.bank).is_err());
    drop(job);
    f.finish().await;
}
#[tokio::test]
async fn clipboard_permission_expiry_resize_and_retirement_do_not_transfer() {
    let f = Fixture::new();
    let key = input_ready(&f).await;
    let id = f.bank.prepare_clipboard(f.nonce, key).unwrap();
    f.bank.clipboard.lock().unwrap().as_mut().unwrap().expires = tokio::time::Instant::now();
    assert!(f.bank.begin_clipboard(f.nonce, id).is_err());
    let id = f.bank.prepare_clipboard(f.nonce, key).unwrap();
    let job = f.bank.begin_clipboard(f.nonce, id).unwrap();
    assert!(job.current(&f.bank).is_ok());
    f.bank
        .entry(f.nonce, key)
        .unwrap()
        .resize_started
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(job.current(&f.bank).is_err());
    f.bank.retire();
    assert!(job.current(&f.bank).is_err());
    drop(job);
    f.finish().await;
}

#[tokio::test]
async fn clipboard_read_cancel_and_shutdown_wait_for_native_job_exit() {
    let f = Fixture::new();
    let key = input_ready(&f).await;
    let id = f.bank.prepare_clipboard(f.nonce, key).unwrap();
    assert!(f.bank.begin_clipboard(f.nonce, Uuid::new_v4()).is_err());
    let job = f.bank.begin_clipboard(f.nonce, id).unwrap();
    assert!(job.current(&f.bank).is_ok());
    assert!(f.bank.begin_clipboard(f.nonce, id).is_err());
    f.bank.cancel_clipboard(f.nonce, id).unwrap();
    assert!(job.current(&f.bank).is_err());
    assert!(matches!(
        f.bank.prepare_clipboard(f.nonce, key),
        Err("workspace.busy")
    ));
    let bank = f.bank.clone();
    let shutdown = tokio::spawn(async move { bank.shutdown().await });
    tokio::task::yield_now().await;
    assert!(!shutdown.is_finished());
    drop(job);
    timeout(Duration::from_secs(2), shutdown)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    f.finish().await;
}
#[tokio::test]
async fn clipboard_read_resize_and_page_change_reject_active_result() {
    let f = Fixture::new();
    let key = input_ready(&f).await;
    let id = f.bank.prepare_clipboard(f.nonce, key).unwrap();
    let job = f.bank.begin_clipboard(f.nonce, id).unwrap();
    let entry = f.bank.entry(f.nonce, key).unwrap();
    entry
        .resize_started
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(job.current(&f.bank).is_err());
    entry
        .resize_started
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let _page = f.registry.activate_launch_page(f.owner).unwrap();
    assert!(job.current(&f.bank).is_err());
    drop(entry);
    drop(job);
    f.finish().await;
}

#[tokio::test]
async fn clipboard_active_expiry_retains_credit_until_native_exit() {
    let f = Fixture::new();
    let key = input_ready(&f).await;
    let id = f.bank.prepare_clipboard(f.nonce, key).unwrap();
    let job = f.bank.begin_clipboard(f.nonce, id).unwrap();
    tokio::time::sleep(Duration::from_millis(5100)).await;
    assert!(job.current(&f.bank).is_err());
    assert!(matches!(
        f.bank.prepare_clipboard(f.nonce, key),
        Err("workspace.busy")
    ));
    drop(job);
    assert!(f.bank.prepare_clipboard(f.nonce, key).is_ok());
    f.finish().await;
}

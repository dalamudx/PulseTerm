#![allow(clippy::unwrap_used)]
use super::super::tests::{application, window};
use super::*;
use std::{future::Future, task::Poll};
use tokio::time::{Duration, timeout};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

async fn native_holder(context: &Workspace) -> TestResultHolder {
    let credit = context.0.profiles.reserve().unwrap();
    let (entered, entering) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let waiting = tokio::spawn(async move {
        tokio::task::spawn_blocking(move || {
            let _credit = credit;
            let _ = entered.send(());
            let _ = released.recv();
        })
        .await
    });
    timeout(Duration::from_secs(2), entering)
        .await
        .unwrap()
        .unwrap();
    waiting.abort();
    assert!(waiting.await.is_err());
    TestResultHolder(Some(release))
}
struct TestResultHolder(Option<std::sync::mpsc::Sender<()>>);
impl Drop for TestResultHolder {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            let _ = release.send(());
        }
    }
}
async fn finish(app: &Arc<Application>) -> TestResult {
    timeout(Duration::from_secs(2), app.window_drains.shutdown()).await??;
    app.profile_storage.shutdown().await?;
    app.settings_io.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn cancelled_shutdown_waiter_still_revokes_all_pages_and_waits_real_native_holder()
-> TestResult {
    let app = application();
    let (a, _, page_a) = window(&app);
    let (b, _, page_b) = window(&app);
    let held = native_holder(&a).await;
    let mut waiting = Box::pin(app.window_drains.shutdown());
    std::future::poll_fn(|cx| {
        assert!(matches!(waiting.as_mut().poll(cx), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    drop(waiting);
    assert!(app.registry.check_launch_page(&page_a).is_err());
    assert!(app.registry.check_launch_page(&page_b).is_err());
    assert!(!app.window_drains.accepting());
    timeout(Duration::from_secs(2), app.window_drains.wait_window(&b)).await??;
    assert!(a.0.retirement.get().unwrap().done.borrow().is_none());
    drop(held);
    finish(&app).await
}
#[tokio::test]
async fn retiring_one_window_keeps_other_page_and_shared_worker_until_explicit_app_exit()
-> TestResult {
    let app = application();
    let (a, _, page_a) = window(&app);
    let (b, _, page_b) = window(&app);
    let held = native_holder(&a).await;
    a.retire_window()?;
    a.retire_window()?;
    assert!(app.registry.check_launch_page(&page_a).is_err());
    app.registry.check_launch_page(&page_b)?;
    assert!(b.0.retirement.get().is_none());
    assert!(app.window_drains.accepting());
    assert!(b.0.profiles.reserve().is_err());
    drop(held);
    timeout(Duration::from_secs(2), app.window_drains.wait_window(&a)).await??;
    drop(b.0.profiles.reserve()?);
    app.registry.check_launch_page(&page_b)?;
    a.retire_window()?;
    finish(&app).await
}
#[tokio::test]
async fn foreign_application_cannot_retire_or_install_same_numeric_window_owner() -> TestResult {
    let a = application();
    let b = application();
    let (one, _, page_one) = window(&a);
    let (two, _, page_two) = window(&b);
    assert!(a.window_drains.install(two.clone()).is_err());
    assert!(a.window_drains.wait_window(&two).await.is_err());
    a.registry.check_launch_page(&page_one)?;
    b.registry.check_launch_page(&page_two)?;
    assert!(one.0.retirement.get().is_none());
    assert!(two.0.retirement.get().is_none());
    finish(&a).await?;
    finish(&b).await?;
    assert!(a.window_drains.wait_window(&two).await.is_err());
    Ok(())
}
#[tokio::test]
async fn failed_revocation_still_drains_other_window_and_cannot_promote_later_shutdown()
-> TestResult {
    let app = application();
    let (a, _, _) = window(&app);
    let (b, _, page_b) = window(&app);
    app.documents.remove(a.0.owner)?;
    assert!(
        timeout(Duration::from_secs(2), app.window_drains.shutdown())
            .await?
            .is_err()
    );
    assert!(app.registry.check_launch_page(&page_b).is_err());
    assert_eq!(*b.0.retirement.get().unwrap().done.borrow(), Some(Ok(())));
    assert!(app.window_drains.shutdown().await.is_err());
    assert!(!app.window_drains.accepting());
    app.profile_storage.shutdown().await?;
    app.settings_io.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn poisoned_supervisor_revokes_all_known_windows_without_allowing_exit() -> TestResult {
    let app = application();
    let (_, _, page_a) = window(&app);
    let (_, _, page_b) = window(&app);
    let manager = app.window_drains.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = manager.state.lock().unwrap();
            panic!("poison metadata only");
        })
        .join()
        .is_err()
    );
    assert!(
        timeout(Duration::from_secs(2), app.window_drains.shutdown())
            .await?
            .is_err()
    );
    assert!(app.registry.check_launch_page(&page_a).is_err());
    assert!(app.registry.check_launch_page(&page_b).is_err());
    assert!(!app.window_drains.accepting());
    app.profile_storage.shutdown().await?;
    app.settings_io.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn thirty_two_slots_keep_retired_holder_until_real_completion_and_reject_reinstall()
-> TestResult {
    let app = application();
    let contexts = (0..32).map(|_| window(&app).0).collect::<Vec<_>>();
    let held = native_holder(&contexts[0]).await;
    contexts[0].retire_window()?;
    let owner = app.registry.open_window()?;
    app.documents.install(owner)?;
    let reader = Arc::new(profiles::ProfileReader::with_storage(
        app.profile_storage.clone(),
    ));
    assert!(Workspace::from_application(app.clone(), owner, reader).is_err());
    app.documents.remove(owner)?;
    app.registry.close_window(owner)?;
    drop(held);
    timeout(
        Duration::from_secs(2),
        app.window_drains.wait_window(&contexts[0]),
    )
    .await??;
    assert!(app.window_drains.install(contexts[0].clone()).is_err());
    let (_, _, page) = window(&app);
    app.registry.check_launch_page(&page)?;
    finish(&app).await
}

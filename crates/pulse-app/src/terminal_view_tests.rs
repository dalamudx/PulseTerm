#![allow(clippy::unwrap_used)]
use super::*;
use pulse_core::TermSize;
use pulse_term::{
    QueryPalette,
    session::{
        self, ChannelTransport, DriverEndpoint, Lifecycle, SessionHandle, SessionTask,
        TransportWrite,
    },
};
use tokio::time::{Duration, timeout};
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Fixture {
    r: SessionRegistry,
    owner: crate::registry::WindowToken,
    page: Arc<PageLaunchScope>,
    h: SessionHandle,
    task: SessionTask,
    driver: DriverEndpoint,
    id: SessionId,
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let r = SessionRegistry::new();
        let owner = r.open_window()?;
        let page = Arc::new(r.activate_launch_page(owner)?);
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (h, task) = session::spawn_session(TermSize::new(32, 8)?, 16, transport);
        let id = r.register(owner, h.clone())?;
        Ok(Self {
            r,
            owner,
            page,
            h,
            task,
            driver,
            id,
        })
    }
    async fn attach(&self) -> Result<AttachedTerminalView, crate::registry::RegistryError> {
        TerminalViewBinding::attach_v2(
            &self.r,
            self.page.clone(),
            self.id,
            AttachmentConfig::default(),
        )
        .await
    }
    async fn finish(self) -> TestResult {
        self.h.close();
        drop(self.driver);
        timeout(Duration::from_secs(5), self.task.join()).await??;
        self.r.reap_closed(self.id)?;
        self.r.shutdown()?;
        Ok(())
    }
}
pub(super) async fn bootstrap(
    view: &mut AttachedTerminalView,
) -> Result<u32, Box<dyn std::error::Error>> {
    loop {
        let wire = timeout(Duration::from_secs(2), view.transport.wire.recv())
            .await?
            .ok_or("bootstrap disconnected")?;
        let frame = pulse_ipc::decode(&wire)?;
        let fragment = pulse_ipc::attach_v2::decode_fragment(frame.payload)?;
        assert_eq!(fragment.generation, view.binding.generation());
        view.binding.acknowledge(frame.sequence)?;
        if fragment.part == pulse_ipc::attach::Part::Commit {
            return Ok(frame.sequence);
        }
    }
}
#[tokio::test]
async fn dropping_current_view_preserves_actor_and_same_page_recovery() -> TestResult {
    let f = Fixture::new()?;
    let mut old = f.attach().await?;
    bootstrap(&mut old).await?;
    let old_generation = old.binding.generation();
    drop(old.binding);
    assert!(
        f.r.check_page_binding(&f.page, f.id, old_generation)
            .is_err()
    );
    assert_eq!(f.h.state.borrow().lifecycle, Lifecycle::Running);
    let _ = timeout(Duration::from_secs(5), old.transport.completion).await?;
    let mut fresh = f.attach().await?;
    bootstrap(&mut fresh).await?;
    fresh.binding.detach()?;
    let _ = timeout(Duration::from_secs(5), fresh.transport.completion).await?;
    f.finish().await
}
#[tokio::test]
async fn refreshed_page_rejects_old_palette_ack_input_and_drop_without_touching_new() -> TestResult
{
    let mut f = Fixture::new()?;
    let mut old = f.attach().await?;
    bootstrap(&mut old).await?;
    f.page = Arc::new(f.r.activate_launch_page(f.owner)?);
    let mut fresh = f.attach().await?;
    bootstrap(&mut fresh).await?;
    assert!(old.binding.check_current().is_err());
    assert!(old.binding.captured_palette().is_err());
    assert!(old.binding.acknowledge(1).is_err());
    assert!(old.binding.input(b"wrong\n".to_vec()).await.is_err());
    assert!(f.driver.input.try_recv().is_err());
    drop(old.binding);
    let _ = timeout(Duration::from_secs(5), old.transport.completion).await?;
    fresh.binding.check_current()?;
    fresh.binding.input(b"new\n".to_vec()).await?;
    assert!(
        matches!(timeout(Duration::from_secs(2), f.driver.input.recv()).await?, Some(TransportWrite::Input(bytes)) if bytes == b"new\n")
    );
    fresh.binding.detach()?;
    let _ = timeout(Duration::from_secs(5), fresh.transport.completion).await?;
    f.finish().await
}
#[tokio::test]
async fn same_page_rebind_drop_and_mutable_metadata_cannot_retarget_binding() -> TestResult {
    let mut f = Fixture::new()?;
    let mut old = f.attach().await?;
    bootstrap(&mut old).await?;
    let mut fresh = f.attach().await?;
    bootstrap(&mut fresh).await?;
    assert_ne!(fresh.binding.generation(), old.binding.generation());
    fresh.transport.generation = old.binding.generation();
    fresh.palette.generation = old.binding.generation();
    assert!(old.binding.input(b"stale".to_vec()).await.is_err());
    drop(old.binding);
    fresh.binding.check_current()?;
    fresh.binding.input(b"ok".to_vec()).await?;
    assert!(
        matches!(timeout(Duration::from_secs(2), f.driver.input.recv()).await?, Some(TransportWrite::Input(bytes)) if bytes == b"ok")
    );
    fresh.binding.detach()?;
    let _ = timeout(Duration::from_secs(5), fresh.transport.completion).await?;
    let _ = timeout(Duration::from_secs(5), old.transport.completion).await?;
    f.finish().await
}
#[tokio::test]
async fn legacy_foreign_window_and_foreign_registry_cannot_borrow_palette_or_detach() -> TestResult
{
    for dual in [false, true] {
        let f = Fixture::new()?;
        let legacy = if dual {
            f.r.attach_v2(f.owner, f.id, AttachmentConfig::default())
                .await?
        } else {
            f.r.attach(f.owner, f.id, AttachmentConfig::default())
                .await?
        };
        assert!(
            f.r.captured_query_palette_page(&f.page, f.id, legacy.generation)
                .is_err()
        );
        assert!(f.r.detach_page(&f.page, f.id, legacy.generation).is_err());
        f.r.check_binding(f.owner, f.id, legacy.generation)?;
        let mut view = f.attach().await?;
        bootstrap(&mut view).await?;
        let foreign_owner = f.r.open_window()?;
        let foreign_page = f.r.activate_launch_page(foreign_owner)?;
        let foreign = SessionRegistry::new();
        let other_owner = foreign.open_window()?;
        let other_page = foreign.activate_launch_page(other_owner)?;
        for page in [&foreign_page, &other_page] {
            assert!(
                f.r.captured_query_palette_page(page, f.id, view.binding.generation())
                    .is_err()
            );
            assert!(
                f.r.detach_page(page, f.id, view.binding.generation())
                    .is_err()
            );
        }
        view.binding.check_current()?;
        view.binding.detach()?;
        let _ = timeout(Duration::from_secs(5), view.transport.completion).await?;
        let _ = timeout(Duration::from_secs(5), legacy.completion).await?;
        foreign.shutdown()?;
        f.finish().await?;
    }
    Ok(())
}
#[tokio::test]
async fn palette_change_revokes_old_view_and_fresh_capture_uses_new_base() -> TestResult {
    let f = Fixture::new()?;
    let a = QueryPalette {
        foreground: [1, 2, 3],
        ..QueryPalette::default()
    };
    f.h.set_query_palette(a.clone()).await?;
    let mut old = f.attach().await?;
    bootstrap(&mut old).await?;
    assert_eq!(old.palette.palette, a);
    let b = QueryPalette {
        foreground: [7, 8, 9],
        ..a
    };
    f.h.set_query_palette(b.clone()).await?;
    assert!(old.binding.captured_palette().is_err());
    let mut fresh = f.attach().await?;
    bootstrap(&mut fresh).await?;
    assert_eq!(fresh.palette.palette, b);
    assert!(fresh.palette.boundary.revision > old.palette.boundary.revision);
    drop(old.binding);
    fresh.binding.check_current()?;
    fresh.binding.detach()?;
    let _ = timeout(Duration::from_secs(5), fresh.transport.completion).await?;
    let _ = timeout(Duration::from_secs(5), old.transport.completion).await?;
    f.finish().await
}
#[path = "terminal_view_delivery_tests.rs"]
mod delivery_tests;

#[tokio::test(flavor = "current_thread")]
async fn cancelled_constructor_before_capture_cannot_install_over_fresh_view() -> TestResult {
    let f = Fixture::new()?;
    let mut pending = Box::pin(f.attach());
    tokio::select! { biased; _ = &mut pending => panic!("capture ran before actor"), _ = std::future::ready(()) => {} }
    drop(pending);
    let mut fresh = f.attach().await?;
    bootstrap(&mut fresh).await?;
    fresh.binding.check_current()?;
    fresh.binding.detach()?;
    let _ = timeout(Duration::from_secs(5), fresh.transport.completion).await?;
    f.finish().await
}

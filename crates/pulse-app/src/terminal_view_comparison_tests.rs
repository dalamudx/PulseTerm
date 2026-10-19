use super::*;
use crate::registry::{RenderedScreen, VisibleComparison};

#[tokio::test]
async fn marker_and_comparison_preserve_state_input_and_existing_quotas() -> TestResult {
    let mut f = Fixture::new()?;
    let mut view = f.attach().await?;
    bootstrap(&mut view).await?;
    view.binding.wait_bootstrap_ready().await?;
    let captured = f.h.diagnostic_digest().await?;
    let rendered = RenderedScreen {
        boundary: captured.boundary.clone(),
        digest: captured.digest,
    };
    view.binding.request_marker().await?;
    let wire = timeout(Duration::from_secs(2), view.transport.wire.recv())
        .await?
        .ok_or("missing marker")?;
    let frame = pulse_ipc::decode(&wire)?;
    let fragment = pulse_ipc::attach_v2::decode_fragment(frame.payload)?;
    assert_eq!(fragment.generation, view.binding.generation());
    assert_eq!(fragment.part, pulse_ipc::attach::Part::Marker);
    assert!(matches!(
        view.binding.request_marker().await,
        Err(crate::registry::RegistryError::Busy)
    ));
    view.binding.acknowledge(frame.sequence)?;
    assert_eq!(
        view.binding.compare_visible(rendered.clone()).await?,
        VisibleComparison::Equal
    );
    assert!(matches!(
        view.binding.compare_visible(rendered.clone()).await,
        Err(crate::registry::RegistryError::Busy)
    ));
    tokio::time::sleep(Duration::from_millis(510)).await;
    let mut bad = rendered;
    bad.digest = "0000000000000000".into();
    assert_eq!(
        view.binding.compare_visible(bad).await?,
        VisibleComparison::Mismatch
    );
    assert_eq!(*f.h.state.borrow(), captured.boundary);
    assert!(f.driver.input.try_recv().is_err());
    view.binding.detach()?;
    let _ = timeout(Duration::from_secs(5), view.transport.completion).await?;
    f.finish().await
}

#[tokio::test]
async fn retired_page_cannot_use_marker_or_comparison_or_charge_the_new_page() -> TestResult {
    let mut f = Fixture::new()?;
    let mut old = f.attach().await?;
    bootstrap(&mut old).await?;
    old.binding.wait_bootstrap_ready().await?;
    let captured = f.h.diagnostic_digest().await?;
    let rendered = RenderedScreen {
        boundary: captured.boundary,
        digest: captured.digest,
    };
    f.page = Arc::new(f.r.activate_launch_page(f.owner)?);
    assert!(old.binding.request_marker().await.is_err());
    assert!(old.binding.compare_visible(rendered.clone()).await.is_err());
    let mut fresh = f.attach().await?;
    bootstrap(&mut fresh).await?;
    fresh.binding.wait_bootstrap_ready().await?;
    fresh.binding.request_marker().await?;
    assert_eq!(
        fresh.binding.compare_visible(rendered).await?,
        VisibleComparison::Equal
    );
    assert!(f.driver.input.try_recv().is_err());
    drop(old.binding);
    let _ = timeout(Duration::from_secs(5), old.transport.completion).await?;
    fresh.binding.detach()?;
    let _ = timeout(Duration::from_secs(5), fresh.transport.completion).await?;
    f.finish().await
}

#[tokio::test]
async fn page_revocation_while_actor_reply_is_pending_wins_for_both_operations() -> TestResult {
    for marker in [true, false] {
        let mut f = Fixture::new()?;
        let mut view = f.attach().await?;
        bootstrap(&mut view).await?;
        view.binding.wait_bootstrap_ready().await?;
        let captured = f.h.diagnostic_digest().await?;
        let rendered = RenderedScreen {
            boundary: captured.boundary,
            digest: captured.digest,
        };
        {
            let pending = async {
                if marker {
                    view.binding
                        .request_marker()
                        .await
                        .map(|()| VisibleComparison::Equal)
                } else {
                    view.binding.compare_visible(rendered).await
                }
            };
            tokio::pin!(pending);
            tokio::select! {
                biased;
                _ = &mut pending => panic!("actor request should be pending"),
                _ = std::future::ready(()) => {}
            }
            let _new_page = f.r.activate_launch_page(f.owner)?;
            assert!(pending.await.is_err());
        }
        assert!(f.driver.input.try_recv().is_err());
        drop(view.binding);
        drop(view.transport.wire);
        let _ = timeout(Duration::from_secs(5), view.transport.completion).await?;
        f.finish().await?;
    }
    Ok(())
}

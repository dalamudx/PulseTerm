#![allow(clippy::unwrap_used)]
use super::*;

#[tokio::test]
async fn failed_send_detaches_even_when_control_survives_without_closing_actor() -> TestResult {
    let f = Fixture::new()?;
    let delivery = TerminalViewDelivery::new(f.attach().await?)?;
    let control = delivery.binding();
    let result = timeout(Duration::from_secs(2), delivery.forward(|_| false)).await?;
    assert!(matches!(result, Err(TerminalDeliveryError::Send)));
    assert!(control.check_current().is_err());
    assert_eq!(f.h.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}

#[tokio::test]
async fn dropping_unpolled_delivery_revokes_lease_despite_retained_control() -> TestResult {
    let f = Fixture::new()?;
    let delivery = TerminalViewDelivery::new(f.attach().await?)?;
    let control = delivery.binding();
    drop(delivery.forward(|_| true));
    assert!(control.check_current().is_err());
    assert_eq!(f.h.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}

#[tokio::test]
async fn cancelling_active_delivery_revokes_lease_despite_retained_control() -> TestResult {
    let f = Fixture::new()?;
    let delivery = TerminalViewDelivery::new(f.attach().await?)?;
    let control = delivery.binding();
    let (sent, entered) = tokio::sync::oneshot::channel();
    let mut sent = Some(sent);
    let task = tokio::spawn(delivery.forward(move |_| {
        if let Some(sent) = sent.take() {
            let _ = sent.send(());
        }
        true
    }));
    timeout(Duration::from_secs(2), entered).await??;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(control.check_current().is_err());
    assert_eq!(f.h.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}

#[tokio::test]
async fn retired_page_never_delivers_even_previously_queued_begin() -> TestResult {
    let f = Fixture::new()?;
    let delivery = TerminalViewDelivery::new(f.attach().await?)?;
    let _fresh_page = f.r.activate_launch_page(f.owner)?;
    let mut delivered = 0;
    assert!(
        timeout(
            Duration::from_secs(2),
            delivery.forward(|_| {
                delivered += 1;
                true
            })
        )
        .await?
        .is_err()
    );
    assert_eq!(delivered, 0);
    assert_eq!(f.h.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}

#[tokio::test]
async fn dropping_old_delivery_never_detaches_replacement() -> TestResult {
    let f = Fixture::new()?;
    let old = TerminalViewDelivery::new(f.attach().await?)?;
    let control = old.binding();
    let mut fresh = f.attach().await?;
    bootstrap(&mut fresh).await?;
    drop(old);
    assert!(control.check_current().is_err());
    fresh.binding.check_current()?;
    fresh.binding.detach()?;
    let _ = timeout(Duration::from_secs(2), fresh.transport.completion).await?;
    f.finish().await
}

#[tokio::test]
async fn mutable_transport_metadata_cannot_select_another_generation() -> TestResult {
    let f = Fixture::new()?;
    let mut view = f.attach().await?;
    view.transport.generation += 1;
    assert!(matches!(
        TerminalViewDelivery::new(view),
        Err(TerminalDeliveryError::Wire)
    ));
    assert_eq!(f.h.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}

#[tokio::test]
async fn admitted_sends_never_supply_ack_credit_or_hide_ack_timeout() -> TestResult {
    let f = Fixture::new()?;
    let view = TerminalViewBinding::attach_v2(
        &f.r,
        f.page.clone(),
        f.id,
        AttachmentConfig {
            frames: 1,
            ack_timeout: Duration::from_millis(100),
            ..AttachmentConfig::default()
        },
    )
    .await?;
    let delivery = TerminalViewDelivery::new(view)?;
    let control = delivery.binding();
    let mut delivered = 0;
    let result = timeout(
        Duration::from_secs(2),
        delivery.forward(|wire| {
            let frame = pulse_ipc::decode(&wire).unwrap();
            let fragment = pulse_ipc::attach_v2::decode_fragment(frame.payload).unwrap();
            assert_eq!(fragment.generation, control.generation());
            delivered += 1;
            true
        }),
    )
    .await?;
    assert_eq!(delivered, 1);
    assert!(matches!(
        result,
        Err(TerminalDeliveryError::Attachment(
            crate::attachment::AttachmentError::AckTimeout
        ))
    ));
    assert!(control.check_current().is_err());
    assert_eq!(f.h.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}

async fn retaining_delivery(
    f: &Fixture,
) -> Result<TerminalViewDelivery, Box<dyn std::error::Error>> {
    Ok(TerminalViewDelivery::new(
        TerminalViewBinding::attach_v2_retaining_ended(
            &f.r,
            f.page.clone(),
            f.id,
            AttachmentConfig::default(),
        )
        .await?,
    )?)
}
#[tokio::test]
async fn retaining_ended_request_still_revokes_on_drop_or_failed_send() -> TestResult {
    for fail_send in [false, true] {
        let f = Fixture::new()?;
        let delivery = retaining_delivery(&f).await?;
        let control = delivery.binding();
        if fail_send {
            let result = timeout(Duration::from_secs(2), delivery.forward(|_| false)).await?;
            assert!(matches!(result, Err(TerminalDeliveryError::Send)));
        } else {
            drop(delivery.forward(|_| true));
        }
        assert!(control.check_current().is_err());
        assert_eq!(f.h.state.borrow().lifecycle, Lifecycle::Running);
        f.finish().await?;
    }
    Ok(())
}

#[tokio::test]
async fn retaining_ended_request_cancelled_while_active_revokes_control() -> TestResult {
    let f = Fixture::new()?;
    let delivery = retaining_delivery(&f).await?;
    let control = delivery.binding();
    let (sent, entered) = tokio::sync::oneshot::channel();
    let mut sent = Some(sent);
    let task = tokio::spawn(delivery.forward(move |_| {
        if let Some(sent) = sent.take() {
            let _ = sent.send(());
        }
        true
    }));
    timeout(Duration::from_secs(2), entered).await??;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(control.check_current().is_err());
    assert_eq!(f.h.state.borrow().lifecycle, Lifecycle::Running);
    f.finish().await
}

#[tokio::test]
async fn completed_delivery_retains_read_scope_only_when_capture_explicitly_opts_in() -> TestResult
{
    for retain in [false, true] {
        let f = Fixture::new()?;
        let delivery = if retain {
            retaining_delivery(&f).await?
        } else {
            TerminalViewDelivery::new(f.attach().await?)?
        };
        let control = delivery.binding();
        f.driver.output.send(b"EOF needle".to_vec()).await?;
        drop(f.driver.output);
        assert_eq!(
            f.task.join_with_receipt().await?.state.lifecycle,
            Lifecycle::Ended
        );
        let receipt = timeout(
            Duration::from_secs(2),
            delivery.forward(|wire| {
                let frame = pulse_ipc::decode(&wire).unwrap();
                control.acknowledge(frame.sequence).unwrap();
                true
            }),
        )
        .await??;
        assert_eq!(receipt.boundary.lifecycle, Lifecycle::Ended);
        assert_eq!(control.check_current().is_ok(), retain);
        if retain {
            assert!(control.wait_bootstrap_ready().await.is_err());
            assert!(control.input(b"never".to_vec()).await.is_err());
            assert!(control.resize(TermSize::new(40, 10)?).await.is_err());
            let request = pulse_term::HistorySearchRequest {
                query: "EOF needle".into(),
                match_case: false,
                regex: false,
                whole_word: false,
                backwards: false,
                alternate: false,
                cursor: None,
                revision: None,
            };
            assert!(
                f.r.history_search(&f.page, f.id, request.clone())
                    .await?
                    .matched_row
                    .is_some()
            );
            let replacement = f.r.activate_launch_page(f.owner)?;
            assert!(control.check_current().is_err());
            assert!(
                f.r.history_search(&f.page, f.id, request.clone())
                    .await
                    .is_err()
            );
            assert!(
                f.r.history_search(&replacement, f.id, request)
                    .await
                    .is_err()
            );
        }
        f.r.close_window(f.owner)?;
    }
    Ok(())
}

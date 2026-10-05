use super::super::tests::{bootstrap, config, session};
use super::*;
use pulse_term::{session::Lifecycle, shell_markers::ShellMarker};
use tokio::time::sleep;
type Error = Box<dyn std::error::Error>;
#[tokio::test]
async fn foreign_and_stale_requests_do_not_drain_or_spend_current_quota() -> Result<(), Error> {
    let registry = SessionRegistry::new();
    let owner = registry.open_window()?;
    let other = registry.open_window()?;
    let (handle, task, mut driver) = session();
    let id = registry.register(owner, handle.clone())?;
    let mut binding = registry.attach(owner, id, config()).await?;
    bootstrap(&registry, owner, id, &mut binding).await;
    driver.output.send(b"\x1b]133;A\x07".to_vec()).await?;
    let wire = binding.wire.recv().await.ok_or("missing data")?;
    let frame = pulse_ipc::decode(&wire)?;
    for (token, generation) in [(other, binding.generation), (owner, binding.generation + 1)] {
        assert!(matches!(
            registry.take_shell_markers(token, id, generation).await,
            Err(RegistryError::Forbidden | RegistryError::Stale)
        ));
    }
    let before = handle.state.borrow().clone();
    let sampled = registry
        .take_shell_markers(owner, id, binding.generation)
        .await?;
    assert_eq!(sampled.hints.markers, [ShellMarker::PromptStart]);
    assert_eq!(*handle.state.borrow(), before);
    assert!(matches!(
        registry
            .take_shell_markers(owner, id, binding.generation)
            .await,
        Err(RegistryError::Busy)
    ));
    registry.acknowledge(owner, id, binding.generation, frame.sequence)?;
    assert!(driver.input.try_recv().is_err());
    registry.shutdown()?;
    drop(driver);
    task.join().await?;
    Ok(())
}
#[tokio::test]
async fn late_response_cannot_cross_rebinding_or_reset_admission() -> Result<(), Error> {
    let registry = SessionRegistry::new();
    let owner = registry.open_window()?;
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle.clone())?;
    let mut old = registry.attach(owner, id, config()).await?;
    bootstrap(&registry, owner, id, &mut old).await;
    let pending = registry.take_shell_markers(owner, id, old.generation);
    tokio::pin!(pending);
    tokio::select! { biased; result=&mut pending => panic!("request must await actor: {result:?}"), _=std::future::ready(())=>{} }
    let mut current = registry.attach(owner, id, config()).await?;
    assert!(matches!(pending.await, Err(RegistryError::Stale)));
    bootstrap(&registry, owner, id, &mut current).await;
    driver.output.send(b"\x1b]133;B\x07".to_vec()).await?;
    let wire = current.wire.recv().await.ok_or("missing data")?;
    let frame = pulse_ipc::decode(&wire)?;
    assert!(matches!(
        registry
            .take_shell_markers(owner, id, current.generation)
            .await,
        Err(RegistryError::Busy)
    ));
    sleep(MIN_INTERVAL).await;
    assert_eq!(
        registry
            .take_shell_markers(owner, id, current.generation)
            .await?
            .hints
            .markers,
        [ShellMarker::PromptEnd]
    );
    registry.acknowledge(owner, id, current.generation, frame.sequence)?;
    registry.close_window(owner)?;
    assert_eq!(handle.state.borrow().lifecycle, Lifecycle::Running);
    handle.close();
    drop(driver);
    task.join().await?;
    Ok(())
}
#[tokio::test]
async fn canceled_request_preserves_hints_and_transfer_preserves_quota() -> Result<(), Error> {
    let registry = SessionRegistry::new();
    let owner = registry.open_window()?;
    let target = registry.open_window()?;
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle.clone())?;
    let mut old = registry.attach(owner, id, config()).await?;
    bootstrap(&registry, owner, id, &mut old).await;
    driver.output.send(b"\x1b]133;D;2\x07".to_vec()).await?;
    let wire = old.wire.recv().await.ok_or("missing data")?;
    let frame = pulse_ipc::decode(&wire)?;
    registry.acknowledge(owner, id, old.generation, frame.sequence)?;
    {
        let pending = registry.take_shell_markers(owner, id, old.generation);
        tokio::pin!(pending);
        tokio::select! { biased; result=&mut pending=>panic!("unexpected reply: {result:?}"),_=std::future::ready(())=>{} }
    }
    handle.inspect().await?;
    registry.transfer(owner, target, id)?;
    let mut current = registry.attach(target, id, config()).await?;
    bootstrap(&registry, target, id, &mut current).await;
    assert!(matches!(
        registry
            .take_shell_markers(target, id, current.generation)
            .await,
        Err(RegistryError::Busy)
    ));
    sleep(MIN_INTERVAL).await;
    assert_eq!(
        registry
            .take_shell_markers(target, id, current.generation)
            .await?
            .hints
            .markers,
        [ShellMarker::CommandEnd {
            exit_status: Some(2)
        }]
    );
    registry.detach(target, id, current.generation)?;
    assert!(matches!(
        registry
            .take_shell_markers(target, id, current.generation)
            .await,
        Err(RegistryError::Stale)
    ));
    registry.shutdown()?;
    drop(driver);
    task.join().await?;
    Ok(())
}

use super::*;
/// Called from the real original-page / unlocked Vault / sealed identity fixture.
pub(crate) async fn verify_bank(
    owner: &PageVaultOwner,
    page: Arc<PageLaunchScope>,
    other: Arc<PageLaunchScope>,
    host: Uuid,
    identity: Uuid,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let capacity = Arc::new(Semaphore::new(1));
    let runtime = tokio::runtime::Handle::current();
    let bank = Bank::new(capacity.clone(), runtime.clone());
    let issued = bank.begin(owner, page.clone(), host, identity)?;
    let id = issued.id;
    let (completed_tx, completed_rx) = oneshot::channel();
    let (finish_tx, finish_rx) = oneshot::channel();
    let prompt = Prompt {
        index: 0,
        count: 2,
        entrance: host.to_string(),
        label: "fixture".into(),
        host: "127.0.0.1".into(),
        port: 22,
        username: "user".into(),
        identity: identity.to_string(),
        identity_label: "fixture identity".into(),
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:fixture".into(),
        first_use: true,
        interactive_available: false,
        key_interactive_available: false,
        agent_interactive_available: false,
        password_interactive_available: false,
    };
    issued.launch(&runtime, move |_, confirm| async move {
        let answer = confirm(prompt).await;
        let _ = completed_tx.send(answer);
        let _ = finish_rx.await;
        Err("workspace.denied")
    });
    assert!(bank.wait(&other, id).await.is_err());
    let reply = bank.wait(&page, id).await?;
    let challenge = match reply {
        Reply::Confirm { challenge, .. } => Uuid::parse_str(&challenge)?,
        _ => return Err("fixture.no_prompt".into()),
    };
    assert!(bank.answer(&page, id, Uuid::new_v4(), true).is_err());
    assert!(bank.answer(&other, id, challenge, true).is_err());
    assert!(bank.wait(&page, id).await.is_err());
    bank.answer(&page, id, challenge, false)?;
    assert!(bank.answer(&page, id, challenge, true).is_err());
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(2), completed_rx)
            .await??
            .map_err(|_| "fixture.answer")?,
        crate::ssh_session_adapter::route::Decision::Approved(false)
    ));
    bank.cancel(&page, id)?;
    assert_eq!(capacity.available_permits(), 0);
    assert!(bank.begin(owner, page.clone(), host, identity).is_err());
    finish_tx.send(()).map_err(|_| "fixture.finish")?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while capacity.available_permits() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let captured = owner.begin_routed_session_gesture_authorized(
        page.clone(),
        host,
        identity,
        Arc::new(|| true),
    )?;
    let issued = bank.begin_from_original(page.clone(), captured)?;
    let id = issued.id;
    let (_finish_tx, finish_rx) = oneshot::channel::<()>();
    issued.launch(&runtime, move |_, _| async move {
        let _ = finish_rx.await;
        Err("workspace.denied")
    });
    let pending = {
        let bank = bank.clone();
        let page = page.clone();
        tokio::spawn(async move { bank.wait(&page, id).await })
    };
    tokio::task::yield_now().await;
    pending.abort();
    let _ = pending.await;
    assert_eq!(capacity.available_permits(), 0);
    bank.retire();
    drop(_finish_tx);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while capacity.available_permits() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    bank.close();
    assert!(bank.begin(owner, page, host, identity).is_err());
    Ok(())
}

/// The ID is from the fixture's real registered SSH terminal, not a fabricated session.
pub(crate) async fn verify_done_consumption(
    owner: &PageVaultOwner,
    page: Arc<PageLaunchScope>,
    host: Uuid,
    identity: Uuid,
    session: SessionId,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let bank = Bank::new(
        Arc::new(Semaphore::new(1)),
        tokio::runtime::Handle::current(),
    );
    let issued = bank.begin(owner, page.clone(), host, identity)?;
    let id = issued.id;
    let consumed = issued.completion_notification();
    issued.launch(&tokio::runtime::Handle::current(), move |_, _| async move {
        Ok(session)
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), consumed.notified())
            .await
            .is_err()
    );
    match bank.wait(&page, id).await? {
        Reply::Connected {
            session: actual, ..
        } => assert_eq!(actual, session.get().to_string()),
        _ => return Err("fixture.no_done".into()),
    }
    // A waiter arriving after wait() returned still gets exactly one stored completion permit.
    tokio::time::timeout(std::time::Duration::from_secs(2), consumed.notified()).await?;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), consumed.notified())
            .await
            .is_err()
    );
    assert!(bank.wait(&page, id).await.is_err());
    bank.close();
    Ok(())
}

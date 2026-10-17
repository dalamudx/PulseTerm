use super::*;
use pulse_app::product_document::ProductDocuments;
use pulse_store::{StateStore, StateStoreTask, ssh_hosts::HostEdit};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn confirming_ticket_cancels_original_native_scope_and_completion_never_cancels_new_token()
-> TestResult {
    let fixture = Fixture::new().await?;
    for cancel in [true, false] {
        let ticket = fixture.ready().await?;
        let token = identity(&ticket.token, 4)?;
        let ready = fixture.bank.observe(&fixture.page, token)?;
        let native = ready.action.authorization();
        if let Some(entry) = fixture
            .bank
            .pending
            .lock()
            .map_err(|_| "fixed lock")?
            .as_mut()
        {
            entry.stage = Stage::Confirming(ready.action.revocation());
        }
        assert!(fixture.bank.observe(&fixture.page, token).is_err());
        assert!(fixture.bank.take_approval(&fixture.page, token).is_err());
        if cancel {
            fixture.bank.cancel(&fixture.page, token)?;
            assert!(!native());
        } else {
            fixture.bank.complete(token)?;
            assert!(native());
        }
        drop(ready);
        assert!(matches!(
            fixture
                .bank
                .actions
                .begin(fixture.page.clone(), fixture.host),
            Err(pulse_app::registry::trust_action::TrustActionError::Busy)
        ));
        drop(native);
        fixture.bank.actions.wait_idle().await?;
    }
    assert_eq!(fixture.store.load_ssh_trust().await?.revision(), 0);
    fixture.bank.shutdown().await?;
    fixture.worker.shutdown().await?;
    Ok(())
}
struct Fixture {
    _temp: tempfile::TempDir,
    store: StateStore,
    worker: StateStoreTask,
    registry: SessionRegistry,
    docs: ProductDocuments,
    window: WindowToken,
    page: Arc<PageLaunchScope>,
    bank: Arc<TrustBank>,
    host: Uuid,
    endpoint: pulse_core::ssh_endpoint::SshEndpoint,
}
impl Fixture {
    async fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let dir = pulse_store::state_directory::prepare(&temp.path().join("state"))?;
        let (store, worker) = StateStore::open(dir.join("state.db")).await?;
        let endpoint =
            pulse_core::ssh_endpoint::SshEndpoint::new("fixed", "127.0.0.1", 2222, "user")?;
        let hosts = store.load_ssh_hosts().await?;
        let hosts = store
            .edit_ssh_hosts(hosts, HostEdit::Create(endpoint.clone()), Arc::new(|| true))
            .await?;
        let host = hosts.records()[0].id();
        let registry = SessionRegistry::new();
        let window = registry.open_window()?;
        let docs = ProductDocuments::new(registry.clone());
        docs.install(window)?;
        docs.started(window)?;
        let nonce = docs.finished(window)?;
        docs.confirm(window, nonce)?;
        let page = docs.current(window, nonce)?;
        let bank = Arc::new(TrustBank::new(registry.clone(), window));
        Ok(Self {
            _temp: temp,
            store,
            worker,
            registry,
            docs,
            window,
            page,
            bank,
            host,
            endpoint,
        })
    }
    async fn ready(&self) -> std::result::Result<TicketDto, Box<dyn std::error::Error>> {
        let action = self.bank.actions.begin(self.page.clone(), self.host)?;
        let source = self
            .store
            .load_ssh_target_source(action.authorization())
            .await?;
        Ok(self.bank.publish(
            self.page.clone(),
            Ready {
                action,
                source,
                endpoint: self.endpoint.clone(),
            },
        )?)
    }
}
#[tokio::test]
async fn ready_ticket_single_use_foreign_page_stage_checks_and_expiry_without_tcp() -> TestResult {
    let fixture = Fixture::new().await?;
    let ticket = fixture.ready().await?;
    let token = identity(&ticket.token, 4)?;
    assert!(identity(&ticket.token.to_uppercase(), 4).is_err());
    assert!(identity(&Uuid::now_v7().to_string(), 4).is_err());
    assert!(fixture.bank.take_approval(&fixture.page, token).is_err());
    let foreign = fixture.registry.open_window()?;
    let other = Arc::new(fixture.registry.activate_launch_page(foreign)?);
    assert!(fixture.bank.observe(&other, token).is_err());
    let ready = fixture.bank.observe(&fixture.page, token)?;
    assert!(fixture.bank.observe(&fixture.page, token).is_err());
    assert!(fixture.bank.take_approval(&fixture.page, token).is_err());
    let native = ready.action.authorization();
    fixture.bank.cancel(&fixture.page, token)?;
    assert!(!native());
    drop(ready);
    assert!(matches!(
        fixture
            .bank
            .actions
            .begin(fixture.page.clone(), fixture.host),
        Err(pulse_app::registry::trust_action::TrustActionError::Busy)
    ));
    drop(native);
    fixture.bank.actions.wait_idle().await?;
    let ticket = fixture.ready().await?;
    let token = identity(&ticket.token, 4)?;
    if let Some(entry) = fixture
        .bank
        .pending
        .lock()
        .map_err(|_| "fixed lock")?
        .as_mut()
    {
        entry.ready_expires = tokio::time::Instant::now();
    }
    assert!(fixture.bank.observe(&fixture.page, token).is_err());
    fixture.bank.remove(token, true);
    fixture.bank.shutdown().await?;
    fixture.worker.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn actual_page_retirement_and_observing_cancel_do_not_release_native_admission_early()
-> TestResult {
    let fixture = Fixture::new().await?;
    let ticket = fixture.ready().await?;
    let token = identity(&ticket.token, 4)?;
    let ready = fixture.bank.observe(&fixture.page, token)?;
    let native = ready.action.authorization();
    let cancel = ready.action.cancellation();
    fixture.docs.started(fixture.window)?;
    fixture.bank.retire();
    cancel.wait().await;
    assert!(!native());
    drop(ready);
    let nonce = fixture.docs.finished(fixture.window)?;
    fixture.docs.confirm(fixture.window, nonce)?;
    let next = fixture.docs.current(fixture.window, nonce)?;
    assert!(fixture.bank.observe(&next, token).is_err());
    assert!(matches!(
        fixture.bank.actions.begin(next.clone(), fixture.host),
        Err(pulse_app::registry::trust_action::TrustActionError::Busy)
    ));
    drop(native);
    fixture.bank.actions.wait_idle().await?;
    let action = fixture.bank.actions.begin(next.clone(), fixture.host)?;
    let source = fixture
        .store
        .load_ssh_target_source(action.authorization())
        .await?;
    let ticket = fixture.bank.publish(
        next.clone(),
        Ready {
            action,
            source,
            endpoint: fixture.endpoint.clone(),
        },
    )?;
    assert!(
        fixture
            .bank
            .cancel(&fixture.page, identity(&ticket.token, 4)?)
            .is_err()
    );
    fixture.bank.cancel(&next, identity(&ticket.token, 4)?)?;
    fixture.bank.shutdown().await?;
    assert!(fixture.registry.check_launch_page(&next).is_ok());
    fixture.worker.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn lost_prepare_receipt_weak_revocation_and_source_changes_never_publish_permission()
-> TestResult {
    let fixture = Fixture::new().await?;
    let action = fixture
        .bank
        .actions
        .begin(fixture.page.clone(), fixture.host)?;
    let source = fixture
        .store
        .load_ssh_target_source(action.authorization())
        .await?;
    let revoke = action.revocation();
    revoke.cancel();
    assert!(
        fixture
            .bank
            .publish(
                fixture.page.clone(),
                Ready {
                    action,
                    source,
                    endpoint: fixture.endpoint.clone()
                }
            )
            .is_err()
    );
    fixture.bank.actions.wait_idle().await?;
    let ticket = fixture.ready().await?;
    fixture
        .bank
        .cancel(&fixture.page, identity(&ticket.token, 4)?)?;
    fixture.bank.shutdown().await?;
    assert_eq!(fixture.store.load_ssh_trust().await?.revision(), 0);
    fixture.worker.shutdown().await?;
    Ok(())
}

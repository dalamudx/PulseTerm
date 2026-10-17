//! Native waiter cancellation and actual CPU/SQL credit supervision; production password work requires a ticket.
#[cfg(test)]
#[path = "product_profiles_vault_tests.rs"]
mod tests;
use super::super::vault::{Intent, Status};
use super::*;
use pulse_app::{
    vault_handoff::VaultHandoffPermit,
    vault_owner::{PageVaultCancellation, PageVaultOwner},
};
struct WaitCancel {
    cancel: PageVaultCancellation,
    armed: bool,
}
impl Drop for WaitCancel {
    fn drop(&mut self) {
        if self.armed {
            self.cancel.cancel();
        }
    }
}
struct Outcome {
    status: Status,
    handed_off: bool,
    #[cfg(test)]
    activation: Option<pulse_app::vault_owner::PageVaultActivation>,
}
impl ProfileReader {
    pub(in super::super) async fn vault(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        owner: PageVaultOwner,
        intent: Intent,
    ) -> Result<Status> {
        self.vault_bound(registry, page, directory, owner, intent, None)
            .await
    }
    pub(in super::super) async fn vault_ticket(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        owner: PageVaultOwner,
        intent: Intent,
        ticket: VaultHandoffPermit,
    ) -> Result<Status> {
        self.vault_bound(registry, page, directory, owner, intent, Some(ticket))
            .await
    }
    async fn vault_bound(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        owner: PageVaultOwner,
        intent: Intent,
        ticket: Option<VaultHandoffPermit>,
    ) -> Result<Status> {
        #[cfg(not(test))]
        if matches!(&intent, Intent::Create(_) | Intent::Unlock(_)) && ticket.is_none() {
            return Err("workspace.vault_denied");
        }
        self.check(&registry, &page)?;
        let credit = self.reserve()?;
        let prepare_credit = credit.clone();
        let reader = self.clone();
        let gate_reader = reader.clone();
        let gate_registry = registry.clone();
        let gate_page = page.clone();
        let ticket_gate = ticket.as_ref().map(|permit| permit.authorization());
        let gate: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
            let _retain_until_native_exit = &credit;
            ticket_gate.as_ref().is_none_or(|gate| gate())
                && gate_reader.check(&gate_registry, &gate_page).is_ok()
        });
        let operation = owner
            .begin_authorized(page.clone(), gate.clone())
            .map_err(|_| "workspace.vault_denied")?;
        if let Some(ticket) = &ticket {
            ticket
                .register_cancel(operation.cancellation())
                .map_err(|_| "workspace.vault_denied")?;
        }
        let mut cancel = WaitCancel {
            cancel: operation.cancellation(),
            armed: true,
        };
        let outcome = tokio::spawn(async move {
            let store = reader
                .prepare_store(&registry, &page, directory, prepare_credit)
                .await?;
            let source = store
                .load_vault_ciphertext(gate.clone())
                .await
                .map_err(|_| "workspace.vault_failed")?;
            let status = match intent {
                Intent::Status => Status::from_key(
                    operation
                        .status(source)
                        .map_err(|_| "workspace.vault_denied")?,
                ),
                Intent::Lock => {
                    let status = operation
                        .status(source)
                        .map_err(|_| "workspace.vault_denied")?;
                    owner.lock().map_err(|_| "workspace.vault_denied")?;
                    Status::from_key(
                        status.map(|_| pulse_store::vault_lifecycle::KeyStatus::Locked),
                    )
                }
                Intent::Create(password) => {
                    #[cfg(debug_assertions)]
                    let started = std::time::Instant::now();
                    let (id, request) = operation
                        .begin_creation(password, source.clone())
                        .map_err(|_| "workspace.vault_failed")?;
                    let ready = request.ready().await.map_err(|_| {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "workspace.vault_creation_cpu_rejected_ms.{}.{}",
                            started.elapsed().as_millis(),
                            gate()
                        );
                        "workspace.vault_failed"
                    })?;
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "workspace.vault_creation_cpu_ready_ms.{}",
                        started.elapsed().as_millis()
                    );
                    operation
                        .persist_creation(&store, source, id, ready)
                        .await
                        .map_err(|_| {
                            #[cfg(debug_assertions)]
                            eprintln!("workspace.vault_creation_persist_rejected.{}", gate());
                            "workspace.vault_failed"
                        })?;
                    Status::from_key(Some(pulse_store::vault_lifecycle::KeyStatus::Locked))
                }
                Intent::Unlock(password) => {
                    let request = operation
                        .begin_unlock(password, source.clone())
                        .map_err(|_| "workspace.vault_failed")?;
                    #[cfg(debug_assertions)]
                    eprintln!("workspace.vault_unlock_admitted");
                    let ready = request
                        .ready()
                        .await
                        .map_err(|_| "workspace.vault_failed")?;
                    let activation = operation
                        .confirm_unlock(&store, source, ready)
                        .await
                        .map_err(|_| "workspace.vault_failed")?;
                    reader.check(&registry, &page)?;
                    if let Some(ticket) = ticket {
                        ticket
                            .publish(activation)
                            .map_err(|_| "workspace.vault_denied")?;
                        return Ok::<Outcome, &'static str>(Outcome {
                            status: Status::from_key(Some(
                                pulse_store::vault_lifecycle::KeyStatus::Pending,
                            )),
                            handed_off: true,
                            #[cfg(test)]
                            activation: None,
                        });
                    }
                    #[cfg(test)]
                    return Ok::<Outcome, &'static str>(Outcome {
                        status: Status::from_key(Some(
                            pulse_store::vault_lifecycle::KeyStatus::Available,
                        )),
                        handed_off: false,
                        activation: Some(activation),
                    });
                    #[cfg(not(test))]
                    return Err("workspace.vault_denied");
                }
            };
            if let Some(ticket) = ticket {
                ticket
                    .finish_creation()
                    .map_err(|_| "workspace.vault_denied")?;
            }
            reader.check(&registry, &page)?;
            Ok::<Outcome, &'static str>(Outcome {
                status,
                handed_off: false,
                #[cfg(test)]
                activation: None,
            })
        })
        .await
        .map_err(|_| "workspace.vault_failed")??;
        if outcome.handed_off {
            cancel.armed = false;
        }
        #[cfg(test)]
        if let Some(activation) = outcome.activation {
            activation.accept().map_err(|_| "workspace.vault_denied")?;
        }
        Ok(outcome.status)
    }
}

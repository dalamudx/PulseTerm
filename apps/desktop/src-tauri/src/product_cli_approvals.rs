//! One original-page GUI approval at a time; no startup or credential authority.
use pulse_app::{registry::PageLaunchScope, window_action::PreparedWindowAction};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
type Result<T> = std::result::Result<T, &'static str>;
pub(super) const WAIT: Duration = Duration::from_secs(30);
struct ActionDelivery {
    action: PreparedWindowAction,
    answer: oneshot::Sender<Result<PreparedWindowAction>>,
}
#[cfg(feature = "workspace-ssh")]
struct IntentDelivery {
    intent: pulse_app::vault_owner::ColdSavedSessionIntent,
    owner: pulse_app::vault_owner::PageVaultOwner,
    answer: oneshot::Sender<Result<pulse_app::vault_owner::SavedSessionGesture>>,
}
#[cfg(feature = "workspace-ssh")]
struct AliasDelivery {
    intent: pulse_app::vault_owner::ColdSavedAliasIntent,
    owner: pulse_app::vault_owner::PageVaultOwner,
    answer: oneshot::Sender<Result<pulse_app::vault_owner::SavedAliasGesture>>,
}
enum Delivery {
    Action(Box<ActionDelivery>),
    #[cfg(feature = "workspace-ssh")]
    Intent(Box<IntentDelivery>),
    #[cfg(feature = "workspace-ssh")]
    Alias(Box<AliasDelivery>),
}
struct Pending {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    stop: CancellationToken,
    until: Instant,
    delivery: Delivery,
}
#[derive(Default)]
pub(crate) struct Approvals(Mutex<Option<Pending>>);
pub(crate) struct Reservation<T = PreparedWindowAction> {
    owner: Arc<Approvals>,
    pub id: Uuid,
    pub answer: oneshot::Receiver<Result<T>>,
}
impl<T> Drop for Reservation<T> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.owner.0.lock()
            && slot.as_ref().is_some_and(|pending| pending.id == self.id)
        {
            slot.take();
        }
    }
}
impl Approvals {
    fn insert<T>(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        stop: CancellationToken,
        delivery: impl FnOnce(oneshot::Sender<Result<T>>) -> Delivery,
    ) -> Result<Reservation<T>> {
        let mut slot = self.0.lock().map_err(|_| "workspace.cli_failed")?;
        if slot.is_some() || stop.is_cancelled() {
            return Err("workspace.cli_busy");
        }
        let id = Uuid::new_v4();
        let (answer, receive) = oneshot::channel();
        *slot = Some(Pending {
            id,
            page,
            stop,
            until: Instant::now() + WAIT,
            delivery: delivery(answer),
        });
        Ok(Reservation {
            owner: self.clone(),
            id,
            answer: receive,
        })
    }
    pub fn reserve(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        stop: CancellationToken,
        action: PreparedWindowAction,
    ) -> Result<Reservation> {
        self.insert(page, stop, |answer| {
            Delivery::Action(Box::new(ActionDelivery { action, answer }))
        })
    }
    #[cfg(feature = "workspace-ssh")]
    pub fn reserve_intent(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        stop: CancellationToken,
        intent: pulse_app::vault_owner::ColdSavedSessionIntent,
        owner: pulse_app::vault_owner::PageVaultOwner,
    ) -> Result<Reservation<pulse_app::vault_owner::SavedSessionGesture>> {
        self.insert(page, stop, |answer| {
            Delivery::Intent(Box::new(IntentDelivery {
                intent,
                owner,
                answer,
            }))
        })
    }
    #[cfg(feature = "workspace-ssh")]
    pub fn reserve_alias(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        stop: CancellationToken,
        intent: pulse_app::vault_owner::ColdSavedAliasIntent,
        owner: pulse_app::vault_owner::PageVaultOwner,
    ) -> Result<Reservation<pulse_app::vault_owner::SavedAliasGesture>> {
        self.insert(page, stop, |answer| {
            Delivery::Alias(Box::new(AliasDelivery {
                intent,
                owner,
                answer,
            }))
        })
    }
    pub fn retire(&self) {
        if let Ok(mut slot) = self.0.lock() {
            slot.take();
        }
    }
    pub fn answer(&self, page: &Arc<PageLaunchScope>, id: Uuid, accept: bool) -> Result<()> {
        let mut slot = self.0.lock().map_err(|_| "workspace.cli_failed")?;
        let pending = slot.as_ref().ok_or("workspace.cli_forbidden")?;
        if pending.id != id || !Arc::ptr_eq(&pending.page, page) {
            return Err("workspace.cli_forbidden");
        }
        let pending = slot.take().ok_or("workspace.cli_forbidden")?;
        if pending.stop.is_cancelled() || Instant::now() >= pending.until {
            return Err("workspace.cli_forbidden");
        }
        match pending.delivery {
            Delivery::Action(delivery) => {
                let result = if accept {
                    delivery
                        .action
                        .confirm(page)
                        .map_err(|_| "workspace.cli_forbidden")
                } else {
                    Err("workspace.cli_cancelled")
                };
                delivery
                    .answer
                    .send(result)
                    .map_err(|_| "workspace.cli_forbidden")
            }
            #[cfg(feature = "workspace-ssh")]
            Delivery::Intent(delivery) => {
                if delivery.answer.is_closed() {
                    return Err("workspace.cli_forbidden");
                }
                // This callback is the separately authenticated, original-page click.
                // Capture here, before returning to an asynchronous waiter.
                let result = if accept {
                    delivery
                        .intent
                        .confirm_authorized(&delivery.owner, page, Arc::new(|| true))
                        .map_err(|_| "workspace.cli_forbidden")
                } else {
                    Err("workspace.cli_cancelled")
                };
                delivery
                    .answer
                    .send(result)
                    .map_err(|_| "workspace.cli_forbidden")
            }
            #[cfg(feature = "workspace-ssh")]
            Delivery::Alias(delivery) => {
                if delivery.answer.is_closed() {
                    return Err("workspace.cli_forbidden");
                }
                let result = if accept {
                    delivery
                        .intent
                        .confirm_authorized(&delivery.owner, page, Arc::new(|| true))
                        .map_err(|_| "workspace.cli_forbidden")
                } else {
                    Err("workspace.cli_cancelled")
                };
                delivery
                    .answer
                    .send(result)
                    .map_err(|_| "workspace.cli_forbidden")
            }
        }
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "product_cli_approval_tests.rs"]
mod tests;

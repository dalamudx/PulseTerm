//! Application-owned window IO supervision. GUI destruction alone is not completion.
use super::*;
use std::collections::HashMap;
use tokio::sync::watch;
#[cfg(all(test, unix))]
#[path = "product_workspace_window_drains_tests.rs"]
mod tests;

pub(crate) struct Retirement {
    revoked: Result<()>,
    done: watch::Receiver<Option<Result<()>>>,
}
struct DrainState {
    closed: bool,
    failed: bool,
    windows: HashMap<WindowToken, Workspace>,
}
pub(crate) struct WindowDrains {
    state: Mutex<DrainState>,
}
impl WindowDrains {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(DrainState {
                closed: false,
                failed: false,
                windows: HashMap::new(),
            }),
        }
    }
    pub(crate) fn accepting(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| !state.closed && !state.failed)
    }
    pub(crate) fn install(self: &Arc<Self>, context: Workspace) -> Result<()> {
        if !Arc::ptr_eq(self, &context.0.window_drains) {
            return Err("workspace.denied");
        }
        let mut state = self.state.lock().map_err(|_| "workspace.shutdown_failed")?;
        if context.0.retirement.get().is_some()
            || state.closed
            || state.failed
            || state.windows.len() >= 32
            || state.windows.contains_key(&context.0.owner)
        {
            return Err("workspace.denied");
        }
        state.windows.insert(context.0.owner, context);
        Ok(())
    }
    pub(crate) fn retire(self: &Arc<Self>, context: &Workspace) -> Result<()> {
        if !Arc::ptr_eq(self, &context.0.window_drains) {
            return Err("workspace.denied");
        }
        if let Some(receipt) = context.0.retirement.get() {
            return receipt.revoked;
        }
        // Poison recovery only revokes known resources; it never admits a new window.
        let (mut state, poisoned) = match self.state.lock() {
            Ok(state) => (state, false),
            Err(poison) => (poison.into_inner(), true),
        };
        if !state
            .windows
            .get(&context.0.owner)
            .is_some_and(|old| Arc::ptr_eq(&old.0, &context.0))
        {
            return Err("workspace.denied");
        }
        let receipt = context.0.retirement.get_or_init(|| {
            let revoked = context.revoke_window().and(if poisoned {
                Err("workspace.shutdown_failed")
            } else {
                Ok(())
            });
            let (done, receiver) = watch::channel(None);
            let context = context.clone();
            let owner = context.0.owner;
            let runtime = context.0.runtime.clone();
            let manager = self.clone();
            let work = runtime.spawn(async move { revoked.and(physical_window(&context).await) });
            runtime.spawn(async move {
                let result = work.await.unwrap_or(Err("workspace.shutdown_failed"));
                {
                    let mut state = manager
                        .state
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    if result.is_ok() {
                        state.windows.remove(&owner);
                    } else {
                        state.failed = true;
                    }
                }
                done.send_replace(Some(result));
            });
            Retirement {
                revoked,
                done: receiver,
            }
        });
        state.failed |= receipt.revoked.is_err();
        receipt.revoked
    }
    /// Revoke every known window, even when another revocation fails.
    pub(crate) fn close(self: &Arc<Self>) -> Result<()> {
        let (contexts, mut result) = match self.state.lock() {
            Ok(mut state) => {
                state.closed = true;
                (state.windows.values().cloned().collect::<Vec<_>>(), Ok(()))
            }
            Err(poison) => {
                let mut state = poison.into_inner();
                state.closed = true;
                state.failed = true;
                (
                    state.windows.values().cloned().collect::<Vec<_>>(),
                    Err("workspace.shutdown_failed"),
                )
            }
        };
        for context in contexts {
            result = result.and(self.retire(&context));
        }
        result
    }
    /// Waiting cancellation cannot abort a window's separately owned native cleanup.
    pub(crate) async fn wait_window(self: &Arc<Self>, context: &Workspace) -> Result<()> {
        if !Arc::ptr_eq(self, &context.0.window_drains) {
            return Err("workspace.denied");
        }
        let revoked = self.retire(context);
        let Some(receipt) = context.0.retirement.get() else {
            return revoked.and(Err("workspace.shutdown_failed"));
        };
        let mut done = receipt.done.clone();
        loop {
            if let Some(result) = *done.borrow_and_update() {
                return result;
            }
            done.changed()
                .await
                .map_err(|_| "workspace.shutdown_failed")?;
        }
    }
    pub(crate) fn native_labels(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .windows
            .values()
            .filter_map(|context| context.0.native.as_ref())
            .filter(|native| native.needs_close())
            .map(|native| native.label().as_str().to_owned())
            .collect()
    }
    pub(crate) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        let mut result = self.close();
        let contexts = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .windows
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for context in contexts {
            result = result.and(self.wait_window(&context).await);
        }
        let failed = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .failed;
        if failed {
            Err("workspace.shutdown_failed")
        } else {
            result
        }
    }
}
async fn physical_window(context: &Workspace) -> Result<()> {
    #[cfg(unix)]
    let profiles = context.0.profiles.drain_window();
    #[cfg(not(unix))]
    let profiles = std::future::ready(Ok::<_, &'static str>(()));
    let (profiles, terminals, settings, keymaps, trust, ssh, visibility) = tokio::join!(
        profiles,
        context.0.terminals.shutdown(),
        context.0.settings.shutdown(),
        context.0.keymaps.shutdown(),
        context.0.ssh_trust.shutdown(),
        context.0.ssh_sessions.shutdown(),
        context.0.visibility.shutdown()
    );
    let result = profiles
        .and(terminals)
        .and(settings)
        .and(keymaps)
        .and(trust)
        .and(ssh)
        .and(visibility.map_err(|_| "workspace.shutdown_failed"));
    if let Some(native) = context.0.native.as_ref() {
        let completion = native.io_finished(result);
        let destroyed = native.wait().await;
        completion.and(destroyed)
    } else {
        result
    }
}

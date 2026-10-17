//! Organization-bound terminal behavior, serialized by the shared SQL worker.
use crate::{
    Result, StateStore, StoreError, catalogue_organization as organization,
    settings::{ResolvedSettings, SettingsPatch, SettingsSource, TerminalType},
    settings_scope::{ScopedSettingsSnapshot, SettingsScope},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;

/// Sparse behavior overrides; null restores inheritance. No connection authority.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overrides {
    /// Validated terminal identifier; None inherits.
    #[serde(deserialize_with = "required_nullable")]
    pub term_type: Option<TerminalType>,
    /// Explicit false is an override; None inherits.
    #[serde(deserialize_with = "required_nullable")]
    pub mac_option_as_meta: Option<bool>,
    /// DEL or BS encoding; None inherits.
    #[serde(deserialize_with = "required_nullable")]
    pub backspace_send: Option<String>,
    /// CSI or DEL encoding; None inherits.
    #[serde(deserialize_with = "required_nullable")]
    pub delete_send: Option<String>,
    /// Fixed terminal base palette; None inherits.
    #[serde(deserialize_with = "required_nullable")]
    pub terminal_preset: Option<crate::settings::TerminalPreset>,
}
fn required_nullable<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(deserializer)
}
impl Overrides {
    fn from_patch(patch: &SettingsPatch) -> Self {
        Self {
            term_type: patch.terminal.term_type,
            mac_option_as_meta: patch.terminal.mac_option_as_meta,
            backspace_send: patch.terminal.backspace_send.clone(),
            delete_send: patch.terminal.delete_send.clone(),
            terminal_preset: patch.terminal.color_preset,
        }
    }
    fn apply(&self, patch: &mut SettingsPatch) -> Result<()> {
        patch.terminal.term_type = self.term_type;
        patch.terminal.mac_option_as_meta = self.mac_option_as_meta;
        patch
            .terminal
            .backspace_send
            .clone_from(&self.backspace_send);
        patch.terminal.delete_send.clone_from(&self.delete_send);
        patch.terminal.color_preset = self.terminal_preset;
        patch.validate().map_err(|_| StoreError::SettingsRecord)
    }
}
/// Private source receipt binds organization, group membership and both setting revisions.
#[derive(Clone)]
pub struct Snapshot {
    organization: organization::Snapshot,
    group: Option<ScopedSettingsSnapshot>,
    target: ScopedSettingsSnapshot,
}
impl Snapshot {
    /// Return only the five overrides owned by this scope.
    pub fn overrides(&self) -> Overrides {
        Overrides::from_patch(&self.target.patch)
    }
    /// Apply the captured direct group and entrance overrides to the global base.
    pub fn resolve(&self, mut base: ResolvedSettings) -> ResolvedSettings {
        for layer in self.group.iter().chain(std::iter::once(&self.target)) {
            let source = match layer.scope() {
                SettingsScope::Group(_) => SettingsSource::Group,
                SettingsScope::Host(_) => SettingsSource::Host,
            };
            if let Some(value) = layer.patch.terminal.term_type {
                base.term_type = crate::settings::Setting { value, source };
            }
            if let Some(value) = layer.patch.terminal.mac_option_as_meta {
                base.mac_option_as_meta = crate::settings::Setting { value, source };
            }
            if let Some(value) = &layer.patch.terminal.backspace_send {
                base.backspace_send = crate::settings::Setting {
                    value: value.clone(),
                    source,
                };
            }
            if let Some(value) = &layer.patch.terminal.delete_send {
                base.delete_send = crate::settings::Setting {
                    value: value.clone(),
                    source,
                };
            }
            if let Some(value) = layer.patch.terminal.color_preset {
                base.terminal_preset = crate::settings::Setting { value, source };
            }
        }
        base
    }
    /// Bounded drawing projection; contains no connection authorization.
    pub fn projection(&self, base: ResolvedSettings) -> serde_json::Value {
        let values = self.resolve(base);
        serde_json::json!({"overrides": self.overrides(), "values": {
            "term_type": values.term_type, "mac_option_as_meta": values.mac_option_as_meta,
            "backspace_send": values.backspace_send, "delete_send": values.delete_send,
            "terminal_preset": values.terminal_preset
        }})
    }
}
pub(crate) struct Operation {
    scope: SettingsScope,
    expected: Option<Snapshot>,
    replacement: Option<Overrides>,
    owner: Uuid,
    authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    reply: oneshot::Sender<Result<Snapshot>>,
}
impl StateStore {
    /// Read a live host/group's behavior in one SQL snapshot; caller supplies original-page guard.
    pub async fn organization_behavior(
        &self,
        scope: SettingsScope,
        expected: Option<Snapshot>,
        replacement: Option<Overrides>,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Snapshot> {
        scope.validate()?;
        if expected.is_some() != replacement.is_some() {
            return Err(StoreError::SettingsRecord);
        }
        let (reply, receive) = oneshot::channel();
        self.admit(crate::Request::OrganizationBehavior(Box::new(Operation {
            scope,
            expected,
            replacement,
            owner: self.host_owner,
            authorize,
            reply,
        })))?;
        receive.await.map_err(|_| StoreError::Closed)?
    }
}
fn load(tx: &Transaction<'_>, owner: Uuid, scope: SettingsScope) -> Result<Snapshot> {
    let organization = organization::load(tx, owner)?;
    let group = organization
        .behavior_group(scope)?
        .map(|id| crate::settings_database::load(tx, SettingsScope::Group(id)))
        .transpose()?;
    let target = crate::settings_database::load(tx, scope)?;
    Ok(Snapshot {
        organization,
        group,
        target,
    })
}
pub(crate) fn serve(connection: &Connection, request: Operation) {
    if request.reply.is_closed() {
        return;
    }
    let value = (|| {
        if !(request.authorize)() {
            return Err(StoreError::ProfileAuthorization);
        }
        let mode = if request.expected.is_some() {
            TransactionBehavior::Immediate
        } else {
            TransactionBehavior::Deferred
        };
        let tx = Transaction::new_unchecked(connection, mode).map_err(|_| StoreError::Database)?;
        let mut current = load(&tx, request.owner, request.scope)?;
        if let (Some(expected), Some(replacement)) = (request.expected, request.replacement) {
            if !current
                .organization
                .behavior_source_matches(&expected.organization)
                || current.target.scope() != expected.target.scope()
                || current.target.revision() != expected.target.revision()
                || current.group.as_ref().map(|s| (s.scope(), s.revision()))
                    != expected.group.as_ref().map(|s| (s.scope(), s.revision()))
            {
                return Err(StoreError::SettingsConflict);
            }
            let mut patch = current.target.patch.clone();
            replacement.apply(&mut patch)?;
            current.target =
                crate::settings_database::write(&tx, request.scope, current.target, patch)?;
        }
        if request.reply.is_closed() || !(request.authorize)() {
            return Err(StoreError::ProfileAuthorization);
        }
        tx.commit().map_err(|_| StoreError::Database)?;
        Ok(current)
    })();
    let _ = request.reply.send(value);
}

#[cfg(test)]
#[path = "organization_behavior_tests.rs"]
mod tests;

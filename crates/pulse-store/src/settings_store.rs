use crate::{
    Request, Result, StateStore, StoreError,
    settings::SettingsPatch,
    settings_scope::{ScopedSettingsEdit, ScopedSettingsSnapshot, SettingsLayers, SettingsScope},
};
use tokio::sync::oneshot;
use uuid::Uuid;
impl StateStore {
    /// Load one validated group/host overlay. Scope IDs confer no entity/window authority.
    pub async fn load_scoped_settings(
        &self,
        scope: SettingsScope,
    ) -> Result<ScopedSettingsSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SettingsLoad(scope, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// One typed mutation of current durable state. Lost replies are not auto-retried;
    /// dropped-before-dispatch requests do not write, executing transactions may commit.
    pub async fn edit_scoped_settings(
        &self,
        scope: SettingsScope,
        edit: ScopedSettingsEdit,
    ) -> Result<ScopedSettingsSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SettingsEdit(scope, edit, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// CAS replacement; an old revision cannot revive after A→B→A or empty tombstones.
    pub async fn replace_scoped_settings(
        &self,
        expected: ScopedSettingsSnapshot,
        patch: SettingsPatch,
    ) -> Result<ScopedSettingsSnapshot> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SettingsReplace(expected, patch, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Read both optional namespaces in one database transaction. Not a permissions check.
    pub async fn load_settings_layers(
        &self,
        group: Option<Uuid>,
        host: Option<Uuid>,
    ) -> Result<SettingsLayers> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::SettingsLayers(group, host, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}

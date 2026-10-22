//! Bounded local app-state storage. Ciphertext-only Vault records, no plaintext or arbitrary SQL IPC.
/// Saved group trees, tags and favorites; metadata only, with no execution authority.
pub mod catalogue_organization;
/// Independently identified local executable Profiles; saved metadata grants no launch permission.
pub mod custom_profiles;
mod database;
/// Native database admission supplied by the trusted upper platform adapter.
pub mod database_admission;
/// Independent host/endpoint/Profile migration and validated snapshots; no launch authority.
pub mod host_catalogue;
#[cfg(test)]
mod identity_io_lifecycle_tests;
mod identity_store;
/// Same-transaction identity metadata and encrypted Vault source, not credential-use authority.
pub mod identity_vault_source;
#[cfg(test)]
mod identity_vault_source_tests;
/// Atomic ciphertext-only private-key/passphrase edit against one original metadata/Vault source.
pub mod key_identity_ciphertext;
#[cfg(test)]
mod key_identity_ciphertext_tests;
mod key_identity_store;
/// Joint instance-bound private-key metadata and encrypted Vault source; no credential-use permission.
pub mod key_identity_vault_source;
/// Bounded workspace keymap metadata; no execution or filesystem authority.
pub mod keymap;
/// Fixed-name, snapshot-safe keymap persistence; not UI or execution authorization.
pub mod keymap_file;
/// Bounded cancellation-safe keymap IO; no page or window permission.
pub mod keymap_io;
mod metadata_batch;
/// Organization-bound behavior editing and frozen startup resolution.
pub mod organization_behavior;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod private_file;
/// Instance-bound default automatic-profile preference; no launch authority.
pub mod profile_default;
/// Joint lifecycle-bound local Profile editor configuration; never launch permission.
pub mod profile_editor;
/// Automatic Unix profile visibility and deletion preferences; no executable paths.
pub mod profiles;
/// Bounded device-local recently registered SSH targets; no connection authority.
pub mod recent_hosts;
/// Pure bounded terminal setting overlays; no file I/O or permissions.
pub mod settings;
mod settings_database;
pub mod settings_edit;
/// Fixed-name private settings snapshots and synchronous atomic saves.
pub mod settings_file;
/// Single-reader last-good settings cache; not a filesystem watcher or UI authority.
pub mod settings_reload;
/// Pure removal of supported overrides, retaining non-target bytes.
pub mod settings_remove;
/// Non-secret group/host sparse settings; scope IDs are not permission tokens.
pub mod settings_scope;
/// Managed single-worker user setting reads; no filesystem watcher or UI authority.
pub mod settings_source;
mod settings_store;
/// Explicit saved Agent public-key metadata; never socket, signing or credential-use authority.
pub mod ssh_agent_identities;
/// Bounded saved SSH endpoint metadata; not credentials, key approval or network authority.
pub mod ssh_hosts;
#[cfg(test)]
mod ssh_hosts_lifecycle_tests;
#[cfg(test)]
mod ssh_hosts_process_tests;
mod ssh_hosts_store;
#[cfg(test)]
mod ssh_hosts_tests;
/// Versioned local SSH identity metadata and sync stamps; not credential-use permission.
pub mod ssh_identities;
#[cfg(test)]
mod ssh_identity_tests;
/// Same-Vault private-key identity metadata with independent ciphertext references and sync stamps.
pub mod ssh_key_identities;
#[cfg(test)]
mod ssh_key_identity_tests;
#[cfg(feature = "ssh-trust")]
/// Same-transaction private-key target metadata/ciphertext/trust; never peer or use authority.
pub mod ssh_key_target_source;
/// Joint instance-bound SSH Profile and identity metadata; never credential-use permission.
pub mod ssh_profiles;
#[cfg(feature = "ssh-trust")]
/// One instance-bound saved jump chain and both identity sources; never peer or execution authority.
pub mod ssh_route_source;
#[cfg(feature = "ssh-trust")]
/// Atomic endpoint/identity/ciphertext/trust source; never target-use or credential authority.
pub mod ssh_target_source;
#[cfg(feature = "ssh-trust")]
/// Optional validated SSH host trust snapshots; no page or approval authority.
pub mod ssh_trust;
#[cfg(not(feature = "ssh-trust"))]
mod ssh_trust;
#[cfg(feature = "ssh-trust")]
mod ssh_trust_store;
#[cfg(all(test, feature = "ssh-trust"))]
mod ssh_trust_tests;
#[cfg(unix)]
/// Prepare a trusted private state directory before opening SQLite; no page path input.
pub mod state_directory;
/// Pure bounded terminal palette import/conversion; no theme-directory or GUI access.
pub mod theme;
/// Backend UUID-named private theme files; no path/write IPC or theme-directory discovery.
pub mod theme_file;
/// Process-wide single-job background theme IO; not window authorization.
pub mod theme_io;
/// Encrypted-only initial Vault creation, bounded CPU scheduling; no plaintext IPC or activation.
pub mod vault_creation;
/// Internal credential AEAD primitive, not a keyring, persistence or plaintext IPC.
pub mod vault_crypto;
/// Bounded ciphertext-only durable Vault captures and CAS intents, no unlock authority.
pub mod vault_database;
#[cfg(test)]
mod vault_database_negative_tests;
#[cfg(test)]
mod vault_database_tests;
#[cfg(test)]
mod vault_envelope_tests;
#[cfg(test)]
mod vault_io_lifecycle_tests;
/// Fixed-cost memory-only master-password validation primitive, not unlock IPC.
pub mod vault_kdf;
/// Backend-only two-phase memory key availability; not frontend authorization.
pub mod vault_lifecycle;
/// Receive-only bounded password ingress; no serialization, key access or CPU admission.
pub mod vault_password_ingress;
/// Receive-only bounded private-key ingress; no serialization, format validation or use authority.
pub mod vault_private_key_ingress;
#[cfg(test)]
mod vault_process_tests;
/// Versioned ciphertext-only credential records; not storage or plaintext IPC authority.
pub mod vault_record;
#[cfg(test)]
mod vault_record_binding_tests;
#[cfg(test)]
mod vault_record_tests;
#[cfg(test)]
mod vault_reference_tests;
#[cfg(test)]
mod vault_relational_tests;
#[cfg(test)]
mod vault_revocation_tests;
#[cfg(test)]
mod vault_source_confirmation_tests;
mod vault_store;
/// Bounded blocking validation unlock jobs; no frontend/password IPC.
pub mod vault_unlock;
/// Fixed-profile DEK wrapping and bounded encrypted envelope codec, not an authorization grant.
pub mod vault_wrap;
use profiles::{AutoProfileEdit, AutoProfilePreferences};
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
/// Maximum requests waiting for the single database worker.
pub const REQUEST_LIMIT: usize = 16;
/// Stable failures never include database paths, SQL, or record contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The caller did not provide a private regular database location.
    #[error("store.private_location_required")]
    Location,
    /// SQLite or filesystem operation failed; no sensitive diagnostic is exposed.
    #[error("store.database_failed")]
    Database,
    /// Migration version is unknown or legacy unversioned tables are present.
    #[error("store.unsupported_schema")]
    Schema,
    /// Existing backup is preserved; migration must not silently overwrite it.
    #[error("store.backup_failed")]
    Backup,
    /// Malformed, oversized, incompatible, duplicated, or conflicting preferences.
    #[error("store.invalid_profile_preferences")]
    Record,
    /// The current preferences differ from the snapshot expected by a replacement.
    #[error("store.profile_preferences_conflict")]
    Conflict,
    /// Scoped settings record, identity, budget or value validation failed.
    #[error("store.invalid_scoped_settings")]
    SettingsRecord,
    /// Expected scope/revision no longer matches current durable state.
    #[error("store.scoped_settings_conflict")]
    SettingsConflict,
    /// Scoped revision must not wrap or reset after deleting the last field.
    #[error("store.scoped_settings_revision_exhausted")]
    SettingsRevision,
    /// No new request was admitted to the bounded worker queue.
    #[error("store.busy")]
    Busy,
    /// Trusted backend execution admission was revoked; the database remains usable.
    #[error("store.profile_edit_denied")]
    ProfileAuthorization,
    /// Database worker has ended; no write retry is implied.
    #[error("store.closed")]
    Closed,
    /// Host trust row is oversized, malformed or from an unsupported schema.
    #[error("store.invalid_ssh_trust_record")]
    HostTrustRecord,
    /// Wrong store owner, stale revision or invalid no-op removal.
    #[error("store.ssh_trust_conflict")]
    HostTrustConflict,
    /// Host trust revision cannot wrap.
    #[error("store.ssh_trust_revision_exhausted")]
    HostTrustRevision,
    /// Saved endpoint catalogue shape, budgets or version is invalid.
    #[error("store.invalid_ssh_host_record")]
    HostRecord,
    /// Wrong store instance, stale snapshot or deleted host identity.
    #[error("store.ssh_host_conflict")]
    HostConflict,
    /// Saved endpoint revision exhausted and must never reset.
    #[error("store.ssh_host_revision_exhausted")]
    HostRevision,
    /// Backend execution admission was revoked before the commit boundary.
    #[error("store.ssh_host_edit_denied")]
    HostAuthorization,
    /// Durable encrypted Vault document is malformed, oversized or unsupported.
    #[error("store.invalid_vault_record")]
    VaultRecord,
    /// Wrong store capture, stale revision or invalid credential/vault transition.
    #[error("store.vault_conflict")]
    VaultConflict,
    /// Vault revision cannot wrap or reset when its last credential is removed.
    #[error("store.vault_revision_exhausted")]
    VaultRevision,
    /// Trusted native read/write admission ended; no unlock or retry is inferred.
    #[error("store.vault_operation_denied")]
    VaultAuthorization,
    /// Malformed/unsupported identity or clock source.
    #[error("store.invalid_identity_record")]
    IdentityRecord,
    /// Foreign/stale capture, missing/deleted identity or wrong Vault reference.
    #[error("store.identity_conflict")]
    IdentityConflict,
    /// Native identity operation authorization was revoked.
    #[error("store.identity_operation_denied")]
    IdentityAuthorization,
    /// Revision/clock/tombstone budget cannot advance without loss.
    #[error("store.identity_exhausted")]
    IdentityExhausted,
    /// Malformed or exhausted organization metadata.
    #[error("store.invalid_organization")]
    OrganizationRecord,
    /// Stale organization or saved-item capture, invalid move or nonempty group.
    #[error("store.organization_conflict")]
    OrganizationConflict,
    /// Original organization operation was revoked.
    #[error("store.organization_denied")]
    OrganizationAuthorization,
    /// Current-epoch key handoff was revoked or belongs to another controller.
    #[error(transparent)]
    VaultLifecycle(#[from] vault_lifecycle::LifecycleError),
}
type Result<T> = std::result::Result<T, StoreError>;
enum Request {
    AgentIdentity(Box<ssh_agent_identities::AgentRequest>),
    RecentHosts(Box<recent_hosts::RecentRequest>),
    Organization(Box<catalogue_organization::OrganizationRequest>),
    ProfileDefault(Box<profile_default::DefaultRequest>),
    ProfileEditor(Box<profile_editor::EditorRequest>),
    CustomProfiles(Box<custom_profiles::CustomRequest>),
    #[cfg(feature = "ssh-trust")]
    SshTargetSource(Box<ssh_target_source::SourceRequest>),
    #[cfg(feature = "ssh-trust")]
    SshKeyTargetSource(Box<ssh_key_target_source::SourceRequest>),
    #[cfg(feature = "ssh-trust")]
    SshRouteSource(Box<ssh_route_source::SourceRequest>),
    IdentityPasswordEdit(Box<identity_vault_source::PasswordEditRequest>),
    KeyIdentityCiphertextEdit(Box<key_identity_ciphertext::KeyIdentityCiphertextRequest>),
    IdentityVaultValidate(
        uuid::Uuid,
        identity_vault_source::IdentityVaultSource,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<()>>,
    ),
    IdentityVaultLoad(
        uuid::Uuid,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<identity_vault_source::IdentityVaultSource>>,
    ),
    IdentityLoad(
        uuid::Uuid,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<ssh_identities::IdentitySnapshot>>,
    ),
    IdentityEdit(
        uuid::Uuid,
        ssh_identities::IdentitySnapshot,
        ssh_identities::IdentityEdit,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<ssh_identities::IdentitySnapshot>>,
    ),
    KeyIdentityLoad(
        uuid::Uuid,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<ssh_key_identities::KeyIdentitySnapshot>>,
    ),
    KeyIdentityEdit(
        uuid::Uuid,
        ssh_key_identities::KeyIdentitySnapshot,
        ssh_key_identities::KeyIdentityEdit,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<ssh_key_identities::KeyIdentitySnapshot>>,
    ),
    KeyIdentityVaultLoad(
        uuid::Uuid,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<key_identity_vault_source::KeyIdentityVaultSource>>,
    ),
    KeyIdentityVaultValidate(
        uuid::Uuid,
        key_identity_vault_source::KeyIdentityVaultSource,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<()>>,
    ),
    VaultConfirm(
        uuid::Uuid,
        vault_database::VaultSnapshot,
        vault_lifecycle::VaultKeyLifecycle,
        vault_unlock::ReadyUnlock,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<vault_lifecycle::UnlockActivation>>,
    ),
    VaultLoad(
        uuid::Uuid,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<vault_database::VaultSnapshot>>,
    ),
    VaultEdit(
        uuid::Uuid,
        vault_database::VaultSnapshot,
        vault_database::VaultEdit,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<vault_database::VaultSnapshot>>,
    ),
    HostsLoad(
        uuid::Uuid,
        Option<std::sync::Arc<dyn Fn() -> bool + Send + Sync>>,
        oneshot::Sender<Result<ssh_hosts::HostSnapshot>>,
    ),
    HostCatalogueLoad(
        uuid::Uuid,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<Option<host_catalogue::Snapshot>>>,
    ),
    HostsEdit(
        uuid::Uuid,
        ssh_hosts::HostSnapshot,
        ssh_hosts::HostEdit,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<ssh_hosts::HostSnapshot>>,
    ),
    SshProfilesLoad(
        uuid::Uuid,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<ssh_profiles::ProfileSnapshot>>,
    ),
    SshProfileEdit(
        uuid::Uuid,
        Box<ssh_profiles::ProfileSnapshot>,
        ssh_profiles::ProfileMutation,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<ssh_profiles::ProfileSnapshot>>,
    ),
    OrganizationBehavior(Box<organization_behavior::Operation>),
    SettingsLoad(
        settings_scope::SettingsScope,
        oneshot::Sender<Result<settings_scope::ScopedSettingsSnapshot>>,
    ),
    SettingsEdit(
        settings_scope::SettingsScope,
        settings_scope::ScopedSettingsEdit,
        oneshot::Sender<Result<settings_scope::ScopedSettingsSnapshot>>,
    ),
    SettingsReplace(
        settings_scope::ScopedSettingsSnapshot,
        settings::SettingsPatch,
        oneshot::Sender<Result<settings_scope::ScopedSettingsSnapshot>>,
    ),
    SettingsLayers(
        Option<uuid::Uuid>,
        Option<uuid::Uuid>,
        oneshot::Sender<Result<settings_scope::SettingsLayers>>,
    ),
    Load(oneshot::Sender<Result<AutoProfilePreferences>>),
    Save(
        AutoProfilePreferences,
        AutoProfilePreferences,
        oneshot::Sender<Result<()>>,
    ),
    Edit(
        AutoProfileEdit,
        oneshot::Sender<Result<AutoProfilePreferences>>,
    ),
    AuthorizedEdit(
        AutoProfileEdit,
        std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        oneshot::Sender<Result<AutoProfilePreferences>>,
    ),
    #[cfg(feature = "ssh-trust")]
    HostTrustLoad(
        uuid::Uuid,
        oneshot::Sender<Result<ssh_trust::TrustSnapshot>>,
    ),
    #[cfg(feature = "ssh-trust")]
    HostTrustEdit(
        uuid::Uuid,
        ssh_trust::TrustSnapshot,
        ssh_trust::TrustEdit,
        oneshot::Sender<Result<ssh_trust::TrustSnapshot>>,
    ),
}
/// Backend-only typed handle. All reads and writes run on one blocking worker.
/// No database access, application directory creation, or native work on UI threads.
#[derive(Clone)]
pub struct StateStore {
    requests: mpsc::Sender<Request>,
    cancellation: CancellationToken,
    host_owner: uuid::Uuid,
    #[cfg(feature = "ssh-trust")]
    trust_owner: uuid::Uuid,
}
/// Trusted worker owner. Drop or explicit cancellation closes the worker even if
/// clients retain handles; already executing database writes cannot be recalled.
pub struct StateStoreTask {
    task: Option<tokio::task::JoinHandle<Result<()>>>,
    cancellation: CancellationToken,
}
type AdmissionFactory = Box<
    dyn FnOnce(&std::path::Path) -> Result<Box<dyn database_admission::DatabaseAdmission>> + Send,
>;
impl StateStore {
    /// Open a database in a caller-provisioned trusted private directory.
    /// Unix requires private directory/file modes and no database symlink; Windows
    /// ACL provisioning is the trusted platform adapter's responsibility.
    pub async fn open(path: PathBuf) -> Result<(Self, StateStoreTask)> {
        Self::open_worker(path, None, Box::new(())).await
    }
    /// Run native admission on the SQLite worker and keep its guard until the connection exits.
    /// The trusted factory must bind the exact path; this does not grant page or credential authority.
    /// Initialization credit stays on the native worker through success or physical failure cleanup.
    pub async fn open_admitted(
        path: PathBuf,
        admit: impl FnOnce(&std::path::Path) -> Result<Box<dyn database_admission::DatabaseAdmission>>
        + Send
        + 'static,
        initialization_credit: impl Send + 'static,
    ) -> Result<(Self, StateStoreTask)> {
        Self::open_worker(path, Some(Box::new(admit)), Box::new(initialization_credit)).await
    }
    async fn open_worker(
        path: PathBuf,
        admit: Option<AdmissionFactory>,
        initialization_credit: Box<dyn Send>,
    ) -> Result<(Self, StateStoreTask)> {
        let (requests, mut inbox) = mpsc::channel(REQUEST_LIMIT);
        let (ready, initialized) = oneshot::channel();
        let (accepted, acceptance) = oneshot::channel();
        let cancellation = CancellationToken::new();
        let worker_cancellation = cancellation.clone();
        let runtime = tokio::runtime::Handle::current();
        let task = tokio::task::spawn_blocking(move || {
            let prepared: Result<_> = (|| {
                let admission = admit.map(|factory| factory(&path)).transpose()?;
                if let Some(guard) = admission.as_deref() {
                    guard.validate()?;
                }
                let connection = match admission.as_deref() {
                    Some(guard) => database::open_admitted(&path, Some(guard))?,
                    None => database::open(&path)?,
                };
                if let Some(guard) = admission.as_deref() {
                    guard.validate()?;
                }
                Ok((connection, admission))
            })();
            let (connection, admission) = match prepared {
                Ok(prepared) => prepared,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return Err(error);
                }
            };
            if ready.send(Ok(())).is_err() || acceptance.blocking_recv().is_err() {
                connection.close().map_err(|_| StoreError::Database)?;
                return Ok(());
            }
            drop(initialization_credit);
            serve(
                connection,
                &mut inbox,
                &worker_cancellation,
                &runtime,
                admission.as_deref(),
            )
        });
        match initialized.await.map_err(|_| StoreError::Closed)? {
            Ok(()) => {
                let result = (
                    Self {
                        requests,
                        host_owner: uuid::Uuid::new_v4(),
                        cancellation: cancellation.clone(),
                        #[cfg(feature = "ssh-trust")]
                        trust_owner: uuid::Uuid::new_v4(),
                    },
                    StateStoreTask {
                        task: Some(task),
                        cancellation,
                    },
                );
                if accepted.send(()).is_err() {
                    result.1.shutdown().await?;
                    return Err(StoreError::Closed);
                }
                Ok(result)
            }
            Err(error) => {
                drop(requests);
                let _ = task.await;
                Err(error)
            }
        }
    }
    fn admit(&self, request: Request) -> Result<()> {
        if self.cancellation.is_cancelled() {
            return Err(StoreError::Closed);
        }
        self.requests.try_send(request).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => StoreError::Busy,
            mpsc::error::TrySendError::Closed(_) => StoreError::Closed,
        })
    }
    /// Load validated preferences. Malformed data never becomes an empty default.
    pub async fn load_auto_profiles(&self) -> Result<AutoProfilePreferences> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::Load(reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Compare expected preferences and atomically replace the row, or return Conflict.
    /// No unconditional snapshot write is available. Dropping the caller
    /// before worker execution cancels it; an already executing write cannot be
    /// recalled. A lost response does not justify an automatic retry.
    pub async fn save_auto_profiles(
        &self,
        expected: AutoProfilePreferences,
        value: AutoProfilePreferences,
    ) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::Save(expected, value, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Apply one typed edit to current durable preferences in an IMMEDIATE transaction.
    /// The result is this edit's committed snapshot, not a guarantee that a later edit
    /// has not already committed. No catalog or session is changed by this API.
    /// Cancel before execution suppresses the edit; already executing writes cannot
    /// be recalled and a lost response must not be automatically retried.
    pub async fn edit_auto_profiles(
        &self,
        edit: AutoProfileEdit,
    ) -> Result<AutoProfilePreferences> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::Edit(edit, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Apply a backend edit only if its short execution-admission predicate remains current.
    /// The worker owns the predicate (and any captured I/O reservation) through actual exit.
    /// Revocation or a dropped receipt before dispatch prevents the write. Once execution
    /// is admitted, an already executing/committed edit is not rolled back by later revocation.
    /// This is not window authority by itself; callers supply a trusted, nonblocking predicate.
    /// There is no frontend predicate, path, SQL, or automatic retry interface.
    pub async fn edit_auto_profiles_authorized(
        &self,
        edit: AutoProfileEdit,
        authorize: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<AutoProfilePreferences> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::AuthorizedEdit(
            edit,
            std::sync::Arc::new(authorize),
            reply,
        ))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
impl StateStoreTask {
    /// Revoke future admission and wake the worker without consuming queue credit.
    /// Pending requests are closed, not replayed; an executing write may still commit.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
    /// Cancel and wait for the actual worker/database closure, retaining no quota.
    pub async fn shutdown(self) -> Result<()> {
        self.cancel();
        self.join().await
    }
    /// Wait for orderly closure. Drop all client handles or first request cancellation.
    /// Cancelling this wait drops the owner and requests shutdown, not a write rollback.
    pub async fn join(mut self) -> Result<()> {
        self.task
            .take()
            .ok_or(StoreError::Closed)?
            .await
            .map_err(|_| StoreError::Closed)?
    }
}
impl Drop for StateStoreTask {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

fn serve(
    connection: rusqlite::Connection,
    inbox: &mut mpsc::Receiver<Request>,
    cancellation: &CancellationToken,
    runtime: &tokio::runtime::Handle,
    admission: Option<&dyn database_admission::DatabaseAdmission>,
) -> Result<()> {
    let mut outcome = Ok(());
    loop {
        let request=runtime.block_on(async {
            tokio::select! { biased; _=cancellation.cancelled()=>None, request=inbox.recv()=>request }
        });
        let Some(request) = request else {
            break;
        };
        if cancellation.is_cancelled() {
            break;
        }
        if let Some(guard) = admission
            && let Err(error) = guard.validate()
        {
            drop(request);
            outcome = Err(error);
            break;
        }
        dispatch(&connection, request);
    }
    connection.close().map_err(|_| StoreError::Database)?;
    if let Some(guard) = admission {
        guard.validate()?;
    }
    outcome
}

fn dispatch(connection: &rusqlite::Connection, request: Request) {
    match request {
        Request::AgentIdentity(request) => request.serve(connection),
        #[cfg(feature = "ssh-trust")]
        Request::SshTargetSource(request) => request.execute(connection),
        #[cfg(feature = "ssh-trust")]
        Request::SshKeyTargetSource(request) => request.execute(connection),
        #[cfg(feature = "ssh-trust")]
        Request::SshRouteSource(request) => request.execute(connection),
        Request::IdentityPasswordEdit(request) => request.execute(connection),
        Request::KeyIdentityCiphertextEdit(request) => request.execute(connection),
        Request::IdentityVaultValidate(owner, expected, authorize, reply) if !reply.is_closed() => {
            let result = identity_vault_source::validate(connection, owner, &expected, || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            });
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::IdentityVaultLoad(owner, authorize, reply) if !reply.is_closed() => {
            let allowed = || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            };
            let result = if allowed() {
                identity_vault_source::load(connection, owner)
            } else {
                Err(StoreError::IdentityAuthorization)
            };
            let result = if allowed() {
                result
            } else {
                Err(StoreError::IdentityAuthorization)
            };
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::IdentityLoad(owner, authorize, reply) if !reply.is_closed() => {
            let allowed = || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            };
            let result = if allowed() {
                ssh_identities::load(connection, owner)
            } else {
                Err(StoreError::IdentityAuthorization)
            };
            let result = if allowed() {
                result
            } else {
                Err(StoreError::IdentityAuthorization)
            };
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::IdentityEdit(owner, expected, intent, authorize, reply) if !reply.is_closed() => {
            let result = ssh_identities::edit(connection, owner, &expected, intent, || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            });
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::KeyIdentityLoad(owner, authorize, reply) if !reply.is_closed() => {
            let allowed = || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            };
            let result = if allowed() {
                ssh_key_identities::load(connection, owner)
            } else {
                Err(StoreError::IdentityAuthorization)
            };
            let result = if allowed() {
                result
            } else {
                Err(StoreError::IdentityAuthorization)
            };
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::KeyIdentityEdit(owner, expected, intent, authorize, reply)
            if !reply.is_closed() =>
        {
            let result = ssh_key_identities::edit(connection, owner, &expected, intent, || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            });
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::KeyIdentityVaultLoad(owner, authorize, reply) if !reply.is_closed() => {
            let allowed = || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            };
            let result = if allowed() {
                key_identity_vault_source::load(connection, owner)
            } else {
                Err(StoreError::IdentityAuthorization)
            };
            let result = if allowed() {
                result
            } else {
                Err(StoreError::IdentityAuthorization)
            };
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::KeyIdentityVaultValidate(owner, expected, authorize, reply)
            if !reply.is_closed() =>
        {
            let result = key_identity_vault_source::validate(connection, owner, &expected, || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            });
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::VaultConfirm(owner, expected, scope, ready, authorize, reply)
            if !reply.is_closed() =>
        {
            let result =
                vault_database::confirm_unlock(connection, owner, &expected, &scope, ready, || {
                    !reply.is_closed()
                        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                            .unwrap_or(false)
                });
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::VaultLoad(owner, authorize, reply) if !reply.is_closed() => {
            let allowed = || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            };
            let value = if allowed() {
                vault_database::load(connection, owner)
            } else {
                Err(StoreError::VaultAuthorization)
            };
            let value = if allowed() {
                value
            } else {
                Err(StoreError::VaultAuthorization)
            };
            drop(authorize);
            let _ = reply.send(value);
        }
        Request::VaultEdit(owner, expected, edit, authorize, reply) if !reply.is_closed() => {
            let value = vault_database::edit(connection, owner, &expected, edit, || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            });
            drop(authorize);
            let _ = reply.send(value);
        }
        Request::HostsLoad(owner, authorize, reply) if !reply.is_closed() => {
            let allowed = || {
                !reply.is_closed()
                    && authorize.as_ref().is_none_or(|gate| {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gate()))
                            .unwrap_or(false)
                    })
            };
            let result = if allowed() {
                ssh_hosts::load(connection, owner)
            } else {
                Err(StoreError::HostAuthorization)
            };
            let result = if allowed() {
                result
            } else {
                Err(StoreError::HostAuthorization)
            };
            // Caller-owned read credit lives inside native work, including a cancelled waiter.
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::HostCatalogueLoad(owner, authorize, reply) if !reply.is_closed() => {
            let allowed = || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            };
            let result = if allowed() {
                host_catalogue::load(connection, owner)
            } else {
                Err(StoreError::HostAuthorization)
            };
            let result = if allowed() {
                result
            } else {
                Err(StoreError::HostAuthorization)
            };
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::HostsEdit(owner, expected, edit, authorize, reply) if !reply.is_closed() => {
            let result = ssh_hosts::edit(connection, owner, &expected, edit, || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            });
            // Keep operation credit through actual SQL exit; release before receipt publication.
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::SshProfilesLoad(owner, authorize, reply) if !reply.is_closed() => {
            let allowed = || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            };
            let result = if allowed() {
                ssh_profiles::load(connection, owner)
            } else {
                Err(StoreError::HostAuthorization)
            };
            let result = if allowed() {
                result
            } else {
                Err(StoreError::HostAuthorization)
            };
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::SshProfileEdit(owner, expected, change, authorize, reply)
            if !reply.is_closed() =>
        {
            let result = ssh_profiles::edit(connection, owner, &expected, change, || {
                !reply.is_closed()
                    && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                        .unwrap_or(false)
            });
            drop(authorize);
            let _ = reply.send(result);
        }
        Request::SettingsLoad(scope, reply) if !reply.is_closed() => {
            let _ = reply.send(settings_database::load(connection, scope));
        }
        Request::SettingsEdit(scope, edit, reply) if !reply.is_closed() => {
            let _ = reply.send(settings_database::edit(connection, scope, edit));
        }
        Request::SettingsReplace(expected, value, reply) if !reply.is_closed() => {
            let _ = reply.send(settings_database::replace(connection, &expected, value));
        }
        Request::SettingsLayers(group, host, reply) if !reply.is_closed() => {
            let _ = reply.send(settings_database::layers(connection, group, host));
        }
        Request::RecentHosts(request) => recent_hosts::serve(connection, *request),
        Request::Organization(request) => catalogue_organization::serve(connection, *request),
        Request::OrganizationBehavior(request) => {
            organization_behavior::serve(connection, *request)
        }
        Request::ProfileDefault(request) => profile_default::serve(connection, *request),
        Request::ProfileEditor(request) => profile_editor::serve(connection, *request),
        Request::CustomProfiles(request) => custom_profiles::serve(connection, *request),
        Request::Load(reply) if !reply.is_closed() => {
            let _ = reply.send(database::load(connection));
        }
        Request::Save(expected, value, reply) if !reply.is_closed() => {
            let _ = reply.send(database::save(connection, &expected, &value));
        }
        Request::Edit(edit, reply) if !reply.is_closed() => {
            let _ = reply.send(database::edit(connection, edit));
        }
        Request::AuthorizedEdit(edit, authorize, reply) if !reply.is_closed() => {
            let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| authorize()))
                .unwrap_or(false);
            let result = if allowed {
                database::edit(connection, edit)
            } else {
                Err(StoreError::ProfileAuthorization)
            };
            // Native SQL work has exited; release operation credit before publishing
            // a receipt whose consumer may immediately admit a fresh read.
            drop(authorize);
            let _ = reply.send(result);
        }

        #[cfg(feature = "ssh-trust")]
        Request::HostTrustLoad(owner, reply) if !reply.is_closed() => {
            let _ = reply.send(ssh_trust::load(connection, owner));
        }
        #[cfg(feature = "ssh-trust")]
        Request::HostTrustEdit(owner, expected, edit, reply) if !reply.is_closed() => {
            let _ = reply.send(ssh_trust::edit(connection, owner, &expected, edit));
        }
        _ => {}
    }
}
#[cfg(test)]
mod authorized_edit_tests;
#[cfg(test)]
mod cancellation_tests;
#[cfg(test)]
mod edit_tests;
#[cfg(all(test, unix))]
mod privacy_tests;
#[cfg(test)]
mod settings_scope_tests;
#[cfg(test)]
mod tests;

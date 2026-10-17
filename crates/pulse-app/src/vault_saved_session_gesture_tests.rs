//! Real persisted roles, single original epoch and ambiguous active-ID rejection.
use super::*;
use crate::vault_owner::{SavedSessionGesture, SavedSessionSelection};
use pulse_core::ssh_identity::SshPasswordIdentity;
use pulse_store::ssh_identities::IdentityEdit;
fn saved(f: &Fixture, identity: Uuid) -> std::result::Result<SavedSessionGesture, VaultOwnerError> {
    f.owner.begin_saved_session_gesture_authorized(
        f.page.scope.clone(),
        f.host,
        identity,
        Arc::new(|| true),
    )
}
async fn password_identity(f: &Fixture) -> std::result::Result<Uuid, Box<dyn std::error::Error>> {
    let secret = Uuid::now_v7();
    f.seal(secret, SecretKind::SshPassword).await?;
    let snapshot = f
        .store
        .edit_ssh_identities(
            f.store.load_ssh_identities(Arc::new(|| true)).await?,
            IdentityEdit::Create(SshPasswordIdentity::new(
                "saved-password",
                "user",
                SecretRef::new(f.vault, secret, SecretKind::SshPassword, false)?,
            )?),
            Arc::new(|| true),
        )
        .await?;
    Ok(snapshot.records().next().ok_or("fixture.password")?.id())
}
#[test]
fn saved_selection_real_roles_presence_and_no_operation_reservation() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f=Fixture::new().await?;
        let capture=saved(&f,f.identity)?;
        drop(f.owner.begin(f.page.scope.clone())?);
        assert!(capture.select_source(&f.source().await?).is_err());
        f.seal(f.key,SecretKind::SshPrivateKey).await?;
        let source=f.source().await?;
        let key=saved(&f,f.identity)?.select_source(&source)?;
        assert!(matches!(&key,SavedSessionSelection::PrivateKey(g) if g.matches_selection(f.host,f.identity)));
        drop(f.owner.begin(f.page.scope.clone())?); drop(key);
        let id=password_identity(&f).await?;
        let source=f.source().await?;
        let password=saved(&f,id)?.select_source(&source)?;
        assert!(matches!(&password,SavedSessionSelection::Password(g) if g.matches_selection(f.host,id)));
        drop(f.owner.begin(f.page.scope.clone())?); drop(password);
        assert!(saved(&f,Uuid::now_v7())?.select_source(&source).is_err());
        let unknown=f.owner.begin_saved_session_gesture_authorized(f.page.scope.clone(),Uuid::now_v7(),f.identity,Arc::new(||true))?;
        assert!(unknown.select_source(&source).is_err());
        let mut expired=saved(&f,f.identity)?; expired.fixture_expire();
        assert!(expired.select_source(&source).is_err());
        f.finish().await
    })
}
#[test]
fn saved_selection_same_dek_reunlock_original_page_and_cancel_cannot_upgrade() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let source = f.source().await?;
        let old = saved(&f, f.identity)?;
        restore_same_dek(&f).await?;
        assert!(old.select_source(&source).is_err());
        let chosen = saved(&f, f.identity)?.select_source(&source)?;
        restore_same_dek(&f).await?;
        let SavedSessionSelection::PrivateKey(key) = chosen else {
            return Err("fixture.kind".into());
        };
        assert!(
            key.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let flag = Arc::new(AtomicBool::new(true));
        let gate = flag.clone();
        let cancelled = f.owner.begin_saved_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(move || gate.load(Ordering::Acquire)),
        )?;
        flag.store(false, Ordering::Release);
        assert!(cancelled.select_source(&source).is_err());
        let old = saved(&f, f.identity)?;
        f.page.documents.started(f.page.window)?;
        f.owner.retire_page()?;
        assert!(old.select_source(&source).is_err());
        let nonce = f.page.documents.finished(f.page.window)?;
        f.page.documents.confirm(f.page.window, nonce)?;
        let current = f.page.documents.current(f.page.window, nonce)?;
        let fresh = f.owner.begin_saved_session_gesture_authorized(
            current,
            f.host,
            f.identity,
            Arc::new(|| true),
        )?;
        f.owner.close()?;
        assert!(fresh.select_source(&source).is_err());
        f.finish().await
    })
}
#[test]
fn saved_selection_real_cross_directory_collision_tombstone_and_source_change() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f=Fixture::new().await?; f.seal(f.key,SecretKind::SshPrivateKey).await?;
        let pw=password_identity(&f).await?;
        // A separate process mutates only this owned fixture DB. No SQL or role comes from the page.
        let status=std::process::Command::new("python3").env("PYTHONDONTWRITEBYTECODE","1").args(["-c","import sqlite3,sys\nwith sqlite3.connect(sys.argv[1]) as c: c.execute('UPDATE identities SET id=? WHERE id=?',(sys.argv[2],sys.argv[3]))"]).arg(f._root.path().join("state/state.db")).arg(f.identity.to_string()).arg(pw.to_string()).status()?;
        assert!(status.success(),"fixture.external_id_collision");
        let source=f.source().await?;
        assert!(saved(&f,f.identity)?.select_source(&source).is_err());
        f.store.edit_ssh_identities(source.password_source().identity_vault().identities().clone(),IdentityEdit::Remove(f.identity),Arc::new(||true)).await?;
        let source=f.source().await?;
        let SavedSessionSelection::PrivateKey(key)=saved(&f,f.identity)?.select_source(&source)? else{return Err("fixture.kind".into());};
        let action=key.begin_session_authorized(&f.owner,&f.page.scope,Arc::new(||true))?;
        f.store.edit_ssh_key_identities(source.identities().clone(),KeyIdentityEdit::Remove(f.identity),Arc::new(||true)).await?;
        assert!(action.prepare(&f.store,source).await.is_err());
        assert!(saved(&f,f.identity)?.select_source(&f.source().await?).is_err());
        f.finish().await
    })
}
#[path = "ssh_jump_gesture_tests.rs"]
mod jump;
#[path = "ssh_route_hop_gesture_tests.rs"]
mod route_hop;
#[path = "vault_saved_agent_gesture_tests.rs"]
mod saved_agent;

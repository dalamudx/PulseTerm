use super::*;
#[test]
fn configured_jump_cannot_be_selected_by_either_direct_saved_identity_gesture() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let password = password_identity(&f).await?;
        for identity in [f.identity, password] {
            assert!(
                saved(&f, identity)?
                    .select_source(&f.source().await?)
                    .is_ok()
            );
        }
        f.store
            .edit_ssh_hosts(
                f.source().await?.hosts().clone(),
                pulse_store::ssh_hosts::HostEdit::Create(
                    pulse_core::ssh_endpoint::SshEndpoint::new(
                        "bastion",
                        "jump.invalid",
                        22,
                        "user",
                    )?,
                ),
                Arc::new(|| true),
            )
            .await?;
        let source = f
            .store
            .load_ssh_profiles_authorized(Arc::new(|| true))
            .await?;
        let leaf = source
            .profiles()
            .find(|p| p.endpoint_id() == f.host)
            .ok_or("fixture.leaf")?
            .metadata()
            .id();
        let via = source
            .profiles()
            .find(|p| p.endpoint_id() != f.host)
            .ok_or("fixture.jump")?
            .metadata()
            .id();
        let routed = f
            .store
            .edit_ssh_profile_jump(source, leaf, Some(via), Arc::new(|| true))
            .await?;
        for identity in [f.identity, password] {
            assert!(
                saved(&f, identity)?
                    .select_source(&f.source().await?)
                    .is_err()
            );
        }
        f.store
            .edit_ssh_profile_jump(routed, leaf, None, Arc::new(|| true))
            .await?;
        for identity in [f.identity, password] {
            assert!(
                saved(&f, identity)?
                    .select_source(&f.source().await?)
                    .is_ok()
            );
        }
        f.finish().await
    })
}

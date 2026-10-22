use super::*;
use crate::{
    registry::tests::{bootstrap, config, session},
    theme_context::tests::{TESTS, document, runtime},
};
use pulse_store::{
    theme_file::ThemeFile,
    theme_io::{ThemeIoError, ThemeIoOwner},
};
use pulse_term::session::{DriverEndpoint, Lifecycle, SessionTask};
use std::os::unix::fs::PermissionsExt;
use uuid::Uuid;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[test]
fn revocation_observation_requires_exact_current_window_generation_and_registry() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.configure()?;
        let p = f.publish().await?;
        assert!(matches!(
            f.registry
                .observe_view_theme_revocation(f.other, f.id, f.binding.generation, &p),
            Err(RegistryThemeError::Registry(RegistryError::Forbidden))
        ));
        assert!(matches!(
            f.registry
                .observe_view_theme_revocation(f.owner, f.id, f.binding.generation + 1, &p),
            Err(RegistryThemeError::Registry(RegistryError::Stale))
        ));
        let foreign = Fixture::new().await?;
        foreign.configure()?;
        let other = foreign.publish().await?;
        assert!(matches!(
            f.registry
                .observe_view_theme_revocation(f.owner, f.id, f.binding.generation, &other),
            Err(RegistryThemeError::Registry(RegistryError::Forbidden))
        ));
        let notice =
            f.registry
                .observe_view_theme_revocation(f.owner, f.id, f.binding.generation, &p)?;
        f.registry
            .invalidate_view_theme(f.owner, f.id, f.binding.generation)?;
        tokio::time::timeout(std::time::Duration::from_secs(1), notice.wait()).await?;
        assert!(matches!(
            f.registry
                .observe_view_theme_revocation(f.owner, f.id, f.binding.generation, &p),
            Err(RegistryThemeError::Registry(RegistryError::Stale))
        ));
        foreign.finish().await?;
        f.finish().await
    })
}
#[test]
fn revocation_observation_wakes_on_navigation_rebind_and_window_close() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        for close in [false, true] {
            let mut f = Fixture::new().await?;
            f.configure()?;
            let p = f.publish().await?;
            let notice = f.registry.observe_view_theme_revocation(
                f.owner,
                f.id,
                f.binding.generation,
                &p,
            )?;
            if close {
                f.registry.close_window(f.owner)?;
            } else {
                assert_eq!(f.registry.retire_window_views(f.owner)?, 1);
            }
            tokio::time::timeout(std::time::Duration::from_secs(1), notice.wait()).await?;
            assert!(f.io.handle().is_open());
            if !close {
                let mut next = f.registry.attach(f.owner, f.id, config()).await?;
                bootstrap(&f.registry, f.owner, f.id, &mut next).await;
                let old = std::mem::replace(&mut f.binding, next);
                let retired =
                    tokio::time::timeout(std::time::Duration::from_secs(1), old.completion)
                        .await??;
                assert!(matches!(
                    retired,
                    Err(crate::attachment::AttachmentError::Detached
                        | crate::attachment::AttachmentError::Invalidated(
                            pulse_term::session::StreamEnd::Detached
                                | pulse_term::session::StreamEnd::Replaced
                        ))
                ));
            }
            f.finish().await?;
        }
        Ok(())
    })
}

struct Fixture {
    dir: tempfile::TempDir,
    io: ThemeIoOwner,
    registry: SessionRegistry,
    owner: WindowToken,
    other: WindowToken,
    id: SessionId,
    binding: RegisteredAttachment,
    actor: SessionTask,
    observer: SessionHandle,
    driver: DriverEndpoint,
}
impl Fixture {
    async fn new() -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let dir = crate::test_tempdir()?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
        let file = ThemeFile::open_private(dir.path().to_path_buf(), Uuid::from_u128(1))?;
        file.save(&file.load()?, &document("initial")?)?;
        let io = ThemeIoOwner::new(file);
        let registry = SessionRegistry::new();
        let owner = registry.open_window()?;
        let other = registry.open_window()?;
        let (handle, actor, driver) = session();
        let observer = handle.clone();
        let id = registry.register(owner, handle)?;
        let mut binding = registry.attach(owner, id, config()).await?;
        bootstrap(&registry, owner, id, &mut binding).await;
        Ok(Self {
            dir,
            io,
            registry,
            owner,
            other,
            id,
            binding,
            actor,
            observer,
            driver,
        })
    }
    fn configure(&self) -> Result<()> {
        self.registry.configure_view_theme(
            self.owner,
            self.id,
            self.binding.generation,
            self.io.handle(),
        )
    }
    async fn publish(&self) -> Result<RegisteredThemeSnapshot> {
        let c = self
            .registry
            .begin_view_theme(self.owner, self.id, self.binding.generation)?
            .ready()
            .await?;
        self.registry
            .confirm_view_theme(self.owner, self.id, self.binding.generation, c)
    }
    async fn finish(self) -> TestResult {
        assert_eq!(self.observer.state.borrow().lifecycle, Lifecycle::Running);
        self.registry.shutdown()?;
        assert_eq!(self.actor.join().await?.lifecycle, Lifecycle::Closed);
        let _exit =
            tokio::time::timeout(std::time::Duration::from_secs(1), self.binding.completion)
                .await?;
        drop(self.driver);
        Ok(())
    }
}
#[test]
fn active_window_close_retires_theme_but_keeps_actor_and_io() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.configure()?;
        let p = f.publish().await?;
        let c = f
            .registry
            .begin_view_theme(f.owner, f.id, f.binding.generation)?
            .ready()
            .await?;
        f.registry.close_window(f.owner)?;
        assert!(matches!(
            f.registry
                .confirm_view_theme(f.owner, f.id, f.binding.generation, c),
            Err(RegistryThemeError::Registry(RegistryError::Forbidden))
        ));
        assert!(matches!(
            f.registry
                .view_theme_current(f.owner, f.id, f.binding.generation, &p),
            Err(RegistryThemeError::Registry(RegistryError::Forbidden))
        ));
        assert!(f.io.handle().is_open());
        f.finish().await
    })
}
#[test]
fn authorization_precedes_unconfigured_and_closed_source() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        assert!(matches!(
            f.registry
                .begin_view_theme(f.owner, f.id, f.binding.generation),
            Err(RegistryThemeError::Unconfigured)
        ));
        f.io.close();
        assert!(matches!(
            f.registry
                .configure_view_theme(f.other, f.id, f.binding.generation, f.io.handle()),
            Err(RegistryThemeError::Registry(RegistryError::Forbidden))
        ));
        assert!(matches!(
            f.registry
                .configure_view_theme(f.owner, f.id, f.binding.generation + 1, f.io.handle()),
            Err(RegistryThemeError::Registry(RegistryError::Stale))
        ));
        assert!(matches!(
            f.configure(),
            Err(RegistryThemeError::Context(ThemeContextError::Io(
                ThemeIoError::Closed
            )))
        ));
        f.finish().await
    })
}
#[test]
fn authorized_snapshot_requires_confirm_and_current_read_is_atomic() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.configure()?;
        let c = f
            .registry
            .begin_view_theme(f.owner, f.id, f.binding.generation)?
            .ready()
            .await?;
        assert!(
            f.registry
                .read_current_view_theme(f.owner, f.id, f.binding.generation)?
                .is_none()
        );
        let p = f
            .registry
            .confirm_view_theme(f.owner, f.id, f.binding.generation, c)?;
        assert!(p.theme().document() == &document("initial")?);
        assert!(
            f.registry
                .view_theme_current(f.owner, f.id, f.binding.generation, &p)?
        );
        let captured = f
            .registry
            .read_current_view_theme(f.owner, f.id, f.binding.generation)?
            .ok_or("missing published theme")?;
        assert!(
            f.registry
                .view_theme_current(f.owner, f.id, f.binding.generation, &captured)?
        );
        f.registry
            .invalidate_view_theme(f.owner, f.id, f.binding.generation)?;
        assert!(
            !f.registry
                .view_theme_current(f.owner, f.id, f.binding.generation, &p)?
        );
        assert!(
            f.registry
                .view_theme_last_good(f.owner, f.id, f.binding.generation)?
                .is_some()
        );
        assert!(
            f.registry
                .read_current_view_theme(f.owner, f.id, f.binding.generation)?
                .is_none()
        );
        f.finish().await
    })
}
#[test]
fn published_theme_palette_uses_confirmed_document_but_cannot_authorize_later_apply() -> TestResult
{
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.configure()?;
        let snapshot = f.publish().await?;
        let expected = crate::terminal_palette::query_palette_from_theme(&document("initial")?);
        assert_eq!(snapshot.terminal_palette(), expected);
        assert!(
            f.registry
                .view_theme_current(f.owner, f.id, f.binding.generation, &snapshot)?
        );
        // Colors are just data; a retained copy does not survive source invalidation.
        let retained = snapshot.terminal_palette();
        f.registry
            .invalidate_view_theme(f.owner, f.id, f.binding.generation)?;
        assert_eq!(snapshot.terminal_palette(), retained);
        assert!(
            !f.registry
                .view_theme_current(f.owner, f.id, f.binding.generation, &snapshot)?
        );
        assert!(
            f.registry
                .read_current_view_theme(f.owner, f.id, f.binding.generation)?
                .is_none()
        );
        assert_eq!(f.observer.state.borrow().lifecycle, Lifecycle::Running);
        f.finish().await
    })
}

#[test]
fn replacement_generation_retires_read_and_snapshot_without_closing_io() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.configure()?;
        let p = f.publish().await?;
        let read = f
            .registry
            .begin_view_theme(f.owner, f.id, f.binding.generation)?;
        let mut next = f.registry.attach(f.owner, f.id, config()).await?;
        bootstrap(&f.registry, f.owner, f.id, &mut next).await;
        assert!(matches!(
            read.ready().await,
            Err(RegistryThemeError::Registry(RegistryError::Stale))
        ));
        assert!(matches!(
            f.registry
                .view_theme_current(f.owner, f.id, f.binding.generation, &p),
            Err(RegistryThemeError::Registry(RegistryError::Stale))
        ));
        assert!(matches!(
            f.registry.begin_view_theme(f.owner, f.id, next.generation),
            Err(RegistryThemeError::Unconfigured)
        ));
        f.registry
            .configure_view_theme(f.owner, f.id, next.generation, f.io.handle())?;
        assert!(
            !f.registry
                .view_theme_current(f.owner, f.id, next.generation, &p)?
        );
        let candidate = f
            .registry
            .begin_view_theme(f.owner, f.id, next.generation)?
            .ready()
            .await?;
        f.registry
            .confirm_view_theme(f.owner, f.id, next.generation, candidate)?;
        assert!(f.io.handle().is_open());
        f.finish().await
    })
}
#[test]
fn navigation_retirement_revokes_candidate_and_new_binding_has_no_theme() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.configure()?;
        let c = f
            .registry
            .begin_view_theme(f.owner, f.id, f.binding.generation)?
            .ready()
            .await?;
        assert_eq!(f.registry.retire_window_views(f.owner)?, 1);
        assert!(matches!(
            f.registry
                .confirm_view_theme(f.owner, f.id, f.binding.generation, c),
            Err(RegistryThemeError::Registry(RegistryError::Stale))
        ));
        let mut next = f.registry.attach(f.owner, f.id, config()).await?;
        bootstrap(&f.registry, f.owner, f.id, &mut next).await;
        assert!(matches!(
            f.registry.begin_view_theme(f.owner, f.id, next.generation),
            Err(RegistryThemeError::Unconfigured)
        ));
        assert!(f.io.handle().is_open());
        f.finish().await
    })
}
#[test]
fn transfer_and_source_window_close_preserve_actor_and_shared_io() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.configure()?;
        let c = f
            .registry
            .begin_view_theme(f.owner, f.id, f.binding.generation)?
            .ready()
            .await?;
        f.registry.transfer(f.owner, f.other, f.id)?;
        assert!(matches!(
            f.registry
                .confirm_view_theme(f.owner, f.id, f.binding.generation, c),
            Err(RegistryThemeError::Registry(RegistryError::Forbidden))
        ));
        f.registry.close_window(f.owner)?;
        let mut next = f.registry.attach(f.other, f.id, config()).await?;
        bootstrap(&f.registry, f.other, f.id, &mut next).await;
        assert!(matches!(
            f.registry.begin_view_theme(f.other, f.id, next.generation),
            Err(RegistryThemeError::Unconfigured)
        ));
        f.registry
            .configure_view_theme(f.other, f.id, next.generation, f.io.handle())?;
        let c = f
            .registry
            .begin_view_theme(f.other, f.id, next.generation)?
            .ready()
            .await?;
        let p = f
            .registry
            .confirm_view_theme(f.other, f.id, next.generation, c)?;
        assert!(
            f.registry
                .view_theme_current(f.other, f.id, next.generation, &p)?
        );
        assert!(f.io.handle().is_open());
        f.finish().await
    })
}
#[test]
fn same_source_noop_preserves_but_same_uuid_new_owner_revokes() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.configure()?;
        let p = f.publish().await?;
        f.configure()?;
        assert!(
            f.registry
                .view_theme_current(f.owner, f.id, f.binding.generation, &p)?
        );
        let c = f
            .registry
            .begin_view_theme(f.owner, f.id, f.binding.generation)?
            .ready()
            .await?;
        let replacement = ThemeIoOwner::new(ThemeFile::open_private(
            f.dir.path().to_path_buf(),
            Uuid::from_u128(1),
        )?);
        f.registry.configure_view_theme(
            f.owner,
            f.id,
            f.binding.generation,
            replacement.handle(),
        )?;
        assert!(
            f.registry
                .view_theme_last_good(f.owner, f.id, f.binding.generation)?
                .is_none()
        );
        assert!(matches!(
            f.registry
                .confirm_view_theme(f.owner, f.id, f.binding.generation, c),
            Err(RegistryThemeError::Context(ThemeContextError::Stale))
        ));
        let _published = f.publish().await?;
        f.finish().await
    })
}
#[test]
fn other_session_candidate_cannot_consume_current_pending_slot() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.configure()?;
        let c = f
            .registry
            .begin_view_theme(f.owner, f.id, f.binding.generation)?
            .ready()
            .await?;
        let (handle, actor, driver) = session();
        let id = f.registry.register(f.owner, handle)?;
        let mut view = f.registry.attach(f.owner, id, config()).await?;
        bootstrap(&f.registry, f.owner, id, &mut view).await;
        f.registry
            .configure_view_theme(f.owner, id, view.generation, f.io.handle())?;
        let pending = f.registry.begin_view_theme(f.owner, id, view.generation)?;
        assert!(matches!(
            f.registry
                .confirm_view_theme(f.owner, id, view.generation, c),
            Err(RegistryThemeError::Registry(RegistryError::Forbidden))
        ));
        assert!(matches!(
            f.registry.begin_view_theme(f.owner, id, view.generation),
            Err(RegistryThemeError::Context(ThemeContextError::Busy))
        ));
        let p =
            f.registry
                .confirm_view_theme(f.owner, id, view.generation, pending.ready().await?)?;
        assert!(
            f.registry
                .view_theme_current(f.owner, id, view.generation, &p)?
        );
        f.finish().await?;
        assert_eq!(actor.join().await?.lifecycle, Lifecycle::Closed);
        drop(driver);
        Ok(())
    })
}
#[test]
fn registry_pointer_is_required_even_with_forced_matching_metadata() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    runtime()?.block_on(async {
        let a = Fixture::new().await?;
        let b = Fixture::new().await?;
        a.configure()?;
        b.configure()?;
        let mut candidate = a
            .registry
            .begin_view_theme(a.owner, a.id, a.binding.generation)?
            .ready()
            .await?;
        candidate.owner = b.owner;
        candidate.id = b.id;
        candidate.generation = b.binding.generation;
        let pending = b
            .registry
            .begin_view_theme(b.owner, b.id, b.binding.generation)?;
        assert!(matches!(
            b.registry
                .confirm_view_theme(b.owner, b.id, b.binding.generation, candidate),
            Err(RegistryThemeError::Registry(RegistryError::Forbidden))
        ));
        let p = b.registry.confirm_view_theme(
            b.owner,
            b.id,
            b.binding.generation,
            pending.ready().await?,
        )?;
        assert!(
            b.registry
                .view_theme_current(b.owner, b.id, b.binding.generation, &p)?
        );
        a.finish().await?;
        b.finish().await
    })
}

//! Fixed private theme fixture behind settings-probe. Actual window binding receipts, not CSS/GUI.
#[cfg(all(test, unix))]
pub(super) static THEME_IO_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[cfg(all(test, unix))]
#[path = "window_theme_probe_tests.rs"]
mod tests;
use super::*;
use pulse_app::{
    registry::{RegisteredThemeCandidate, RegisteredThemeSnapshot, RegistryThemeError},
    theme_context::ThemeContextError,
};
use pulse_store::{
    theme::{
        AccentChoice, NeutralScale, ThemeAppearance, ThemeDocument, ThemeImportFormat, UiThemeBase,
        import_theme,
    },
    theme_file::ThemeFile,
    theme_io::ThemeIoOwner,
};
use std::path::PathBuf;
pub(super) struct Fixture {
    a: ThemeIoOwner,
    b: ThemeIoOwner,
    documents: [ThemeDocument; 3],
    initial: RegisteredThemeSnapshot,
    published: RegisteredThemeSnapshot,
    pending: Option<RegisteredThemeCandidate>,
    owner: WindowToken,
    generation: u64,
    refreshed: bool,
}
fn require(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err("session.native_theme_invalid".into())
    }
}
fn document(
    name: &str,
    color: &str,
    appearance: ThemeAppearance,
    ui_background: &str,
) -> Result<ThemeDocument> {
    let keys = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "purple",
        "cyan",
        "white",
        "brightBlack",
        "brightRed",
        "brightGreen",
        "brightYellow",
        "brightBlue",
        "brightPurple",
        "brightCyan",
        "brightWhite",
    ];
    let fields = keys
        .into_iter()
        .map(|key| format!("\"{key}\":\"{color}\""))
        .collect::<Vec<_>>()
        .join(",");
    let imported = import_theme(
        ThemeImportFormat::WindowsTerminal,
        &format!(
            "{{\"name\":\"{name}\",\"foreground\":\"#ffffff\",\"background\":\"#000000\",{fields}}}"
        ),
        None,
    )
    .map_err(error)?;
    let document = ThemeDocument::from_import(
        &imported,
        appearance,
        UiThemeBase::new(
            NeutralScale::Slate,
            AccentChoice::parse("cyan").map_err(error)?,
        ),
    )
    .map_err(error)?;
    // Explicit theme overrides from Radix3 light step12. Do not silently repair step11 colors.
    let foreground_overrides = if appearance == ThemeAppearance::Light {
        "\"--accent-fg\"=\"#0d3c48\"\n\"--warning-fg\"=\"#4f3422\"\n\"--success-fg\"=\"#193b2d\"\n\"--info-fg\"=\"#113264\"\n"
    } else {
        ""
    };
    pulse_store::theme::parse_theme_document(&format!(
        "{}\n[ui.overrides]\n\"--bg-base\"=\"{ui_background}\"\n{foreground_overrides}",
        document.to_toml().map_err(error)?
    ))
    .map_err(error)
}
pub(super) fn files(
    directory: PathBuf,
) -> Result<(ThemeIoOwner, ThemeIoOwner, [ThemeDocument; 3])> {
    let documents = [
        document("native-first", "#112233", ThemeAppearance::Light, "#f9fafb")?,
        document(
            "native-refreshed",
            "#445566",
            ThemeAppearance::Dark,
            "#243546",
        )?,
        document(
            "native-updated",
            "#778899",
            ThemeAppearance::Dark,
            "#223344",
        )?,
    ];
    let a = ThemeFile::open_private(directory.clone(), uuid::Uuid::from_u128(11)).map_err(error)?;
    let b = ThemeFile::open_private(directory, uuid::Uuid::from_u128(12)).map_err(error)?;
    a.save(&a.load().map_err(error)?, &documents[0])
        .map_err(error)?;
    b.save(&b.load().map_err(error)?, &documents[1])
        .map_err(error)?;
    Ok((ThemeIoOwner::new(a), ThemeIoOwner::new(b), documents))
}
async fn publish(
    registry: &SessionRegistry,
    owner: WindowToken,
    id: SessionId,
    generation: u64,
    expected: &ThemeDocument,
) -> Result<RegisteredThemeSnapshot> {
    let candidate = registry
        .begin_view_theme(owner, id, generation)
        .map_err(error)?
        .ready()
        .await
        .map_err(error)?;
    let snapshot = registry
        .confirm_view_theme(owner, id, generation, candidate)
        .map_err(error)?;
    require(
        snapshot.theme().document() == expected
            && registry
                .view_theme_current(owner, id, generation, &snapshot)
                .map_err(error)?,
    )?;
    let current = registry
        .read_current_view_theme(owner, id, generation)
        .map_err(error)?
        .ok_or("session.native_theme_current_missing")?;
    require(
        current.theme().document() == expected
            && registry
                .view_theme_current(owner, id, generation, &current)
                .map_err(error)?,
    )?;
    Ok(snapshot)
}
impl Fixture {
    pub(super) async fn prepare(
        registry: &SessionRegistry,
        owner: WindowToken,
        other: WindowToken,
        id: SessionId,
        generation: u64,
        directory: PathBuf,
    ) -> Result<Self> {
        let (a, b, documents) = tokio::task::spawn_blocking(move || files(directory))
            .await
            .map_err(error)??;
        require(matches!(
            registry.begin_view_theme(owner, id, generation),
            Err(RegistryThemeError::Unconfigured)
        ))?;
        require(matches!(
            registry.configure_view_theme(other, id, generation, a.handle()),
            Err(RegistryThemeError::Registry(
                pulse_app::registry::RegistryError::Forbidden
            ))
        ))?;
        let wrong = generation
            .checked_add(1)
            .ok_or("session.native_theme_generation_exhausted")?;
        require(matches!(
            registry.configure_view_theme(owner, id, wrong, a.handle()),
            Err(RegistryThemeError::Registry(
                pulse_app::registry::RegistryError::Stale
            ))
        ))?;
        registry
            .configure_view_theme(owner, id, generation, a.handle())
            .map_err(error)?;
        let published = publish(registry, owner, id, generation, &documents[0]).await?;
        let pending = Some(
            registry
                .begin_view_theme(owner, id, generation)
                .map_err(error)?
                .ready()
                .await
                .map_err(error)?,
        );
        eprintln!("session.native_theme.initial_checked");
        Ok(Self {
            a,
            b,
            documents,
            initial: published.clone(),
            published,
            pending,
            owner,
            generation,
            refreshed: false,
        })
    }
    pub(super) async fn refreshed(
        &mut self,
        registry: &SessionRegistry,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<()> {
        require(!self.refreshed && owner == self.owner && generation > self.generation)?;
        require(matches!(
            registry.view_theme_current(owner, id, self.generation, &self.published),
            Err(RegistryThemeError::Registry(
                pulse_app::registry::RegistryError::Stale
            ))
        ))?;
        let candidate = self
            .pending
            .take()
            .ok_or("session.native_theme_pending_missing")?;
        require(matches!(
            registry.confirm_view_theme(owner, id, self.generation, candidate),
            Err(RegistryThemeError::Registry(
                pulse_app::registry::RegistryError::Stale
            ))
        ))?;
        require(self.a.handle().is_open() && self.b.handle().is_open())?;
        require(matches!(
            registry.begin_view_theme(owner, id, generation),
            Err(RegistryThemeError::Unconfigured)
        ))?;
        registry
            .configure_view_theme(owner, id, generation, self.b.handle())
            .map_err(error)?;
        require(
            !registry
                .view_theme_current(owner, id, generation, &self.initial)
                .map_err(error)?,
        )?;
        self.published = publish(registry, owner, id, generation, &self.documents[1]).await?;
        self.pending = Some(
            registry
                .begin_view_theme(owner, id, generation)
                .map_err(error)?
                .ready()
                .await
                .map_err(error)?,
        );
        self.generation = generation;
        self.refreshed = true;
        eprintln!("session.native_theme.reload_checked");
        Ok(())
    }
    // Called inside the existing target render guard after actual Channel draw/ACK.
    pub(super) async fn transferred(
        mut self,
        state: &WindowSessions,
        window: &WebviewWindow,
        nonce: &str,
        owner: WindowToken,
        id: SessionId,
        generation: u64,
    ) -> Result<()> {
        let registry = &state.0.registry;
        require(self.refreshed && owner != self.owner && generation > self.generation)?;
        require(matches!(
            registry.view_theme_current(self.owner, id, self.generation, &self.published),
            Err(RegistryThemeError::Registry(
                pulse_app::registry::RegistryError::Forbidden
            ))
        ))?;
        let candidate = self
            .pending
            .take()
            .ok_or("session.native_theme_pending_missing")?;
        require(matches!(
            registry.confirm_view_theme(self.owner, id, self.generation, candidate),
            Err(RegistryThemeError::Registry(
                pulse_app::registry::RegistryError::Forbidden
            ))
        ))?;
        require(self.a.handle().is_open() && self.b.handle().is_open())?;
        require(matches!(
            registry.begin_view_theme(owner, id, generation),
            Err(RegistryThemeError::Unconfigured)
        ))?;
        registry
            .configure_view_theme(owner, id, generation, self.a.handle())
            .map_err(error)?;
        require(
            !registry
                .view_theme_current(owner, id, generation, &self.initial)
                .map_err(error)?
                && !registry
                    .view_theme_current(owner, id, generation, &self.published)
                    .map_err(error)?,
        )?;
        let captured = publish(registry, owner, id, generation, &self.documents[0]).await?;
        let stale = registry
            .begin_view_theme(owner, id, generation)
            .map_err(error)?
            .ready()
            .await
            .map_err(error)?;
        let source = self.a.handle();
        let expected = source
            .begin_load()
            .map_err(error)?
            .ready()
            .await
            .map_err(error)?;
        let committed = source
            .begin_save(expected, self.documents[2].clone())
            .map_err(error)?
            .ready()
            .await
            .map_err(error)?;
        require(committed.document().map_err(error)?.as_ref() == Some(&self.documents[2]))?;
        registry
            .invalidate_view_theme(owner, id, generation)
            .map_err(error)?;
        require(matches!(
            registry.confirm_view_theme(owner, id, generation, stale),
            Err(RegistryThemeError::Context(ThemeContextError::Stale))
        ))?;
        require(
            !registry
                .view_theme_current(owner, id, generation, &captured)
                .map_err(error)?
                && registry
                    .read_current_view_theme(owner, id, generation)
                    .map_err(error)?
                    .is_none(),
        )?;
        let fresh = publish(registry, owner, id, generation, &self.documents[2]).await?;
        theme_target::deliver(state, window, nonce, owner, id, generation).await?;
        self.a.close();
        self.b.close();
        require(
            !registry
                .view_theme_current(owner, id, generation, &fresh)
                .map_err(error)?
                && registry
                    .read_current_view_theme(owner, id, generation)
                    .map_err(error)?
                    .is_none(),
        )?;
        require(registry.state(owner, id).map_err(error)?.lifecycle == Lifecycle::Running)?;
        eprintln!("session.native_theme.target_checked");
        Ok(())
    }
}

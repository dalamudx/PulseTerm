//! Fixed config directory, page-bound opaque snapshots, and physical blocking-I/O ownership.
use super::*;
#[path = "product_settings_io.rs"]
mod shared_io;
#[cfg(all(test, unix))]
#[path = "product_settings_tests.rs"]
mod tests;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
struct SystemAppearance(Arc<AtomicBool>);

// Tao 0.37 caches the initial portal theme in Window::theme; GTK receives later signals.
#[cfg(target_os = "linux")]
pub(super) fn observe_system_appearance(app: &tauri::App) -> Result<()> {
    use gtk::prelude::*;
    if !gtk::is_initialized_main_thread() {
        return Err("workspace.settings_unavailable");
    }
    let current = Arc::new(AtomicBool::new(false));
    if !app.manage(SystemAppearance(current.clone())) {
        return Err("workspace.settings_unavailable");
    }
    if let Some(settings) = gtk::Settings::default() {
        current.store(
            settings.is_gtk_application_prefer_dark_theme(),
            Ordering::Release,
        );
        let current = Arc::downgrade(&current);
        settings.connect_gtk_application_prefer_dark_theme_notify(move |settings| {
            if let Some(current) = current.upgrade() {
                current.store(
                    settings.is_gtk_application_prefer_dark_theme(),
                    Ordering::Release,
                );
            }
        });
    }
    Ok(())
}

/// Read the native system appearance; an unavailable query uses light.
pub(super) fn system_dark(window: &tauri::WebviewWindow) -> bool {
    #[cfg(target_os = "linux")]
    {
        window
            .try_state::<SystemAppearance>()
            .is_some_and(|current| current.0.load(Ordering::Acquire))
    }
    #[cfg(not(target_os = "linux"))]
    {
        window
            .theme()
            .is_ok_and(|theme| theme == tauri::Theme::Dark)
    }
}
use pulse_app::registry::PageLaunchScope;
use pulse_store::{
    settings::{ResolvedSettings, SettingsResolver},
    settings_edit::TerminalSettingEdit as Edit,
    settings_file::{SettingsFile, SettingsMutation, SettingsSnapshot},
    settings_remove::TerminalSettingKey as Key,
};
pub(super) use shared_io::SettingsIo;
use std::{path::PathBuf, sync::Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};
#[derive(serde::Deserialize)]
#[serde(
    tag = "field",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum Change {
    TerminalTheme(pulse_store::settings::TerminalTheme),
    ResetTerminalTheme,
    HistoryLimit(u32),
    ResetHistoryLimit,
    FontSize(u16),
    LineHeightPercent(u16),
    LetterSpacing(u16),
    FontFamily(String),
    FontFallback(String),
    FontLigatures(bool),
    CursorBlink(bool),
    ScrollOnInput(bool),
    ScrollOnOutput(bool),
    MacOptionAsMeta(bool),
    ResetMacOptionAsMeta,
    BackspaceSend(String),
    DeleteSend(String),
    ResetDeleteSend,
    TermType(pulse_store::settings::TerminalType),
    ResetTermType,
    ResetBackspaceSend,
    ResetScrollOnInput,
    ResetScrollOnOutput,
    CopyOnSelect(bool),
    RightClickPaste(bool),
    UiAppearance(pulse_store::settings::UiAppearance),
    UiAccent(pulse_store::settings::UiAccent),
    ResetUiAccent,
    UiLanguage(pulse_store::settings::UiLanguage),
    ResetUiLanguage,
    TerminalPreset(pulse_store::settings::TerminalPreset),
    ResetTerminalPreset,
    ResetUiAppearance,
    ResetFontSize,
    ResetLineHeightPercent,
    ResetLetterSpacing,
    ResetFontFamily,
    ResetFontFallback,
    ResetFontLigatures,
    ResetCursorBlink,
    ResetCopyOnSelect,
    ResetRightClickPaste,
}
impl Change {
    fn mutation(self) -> SettingsMutation {
        match self {
            Self::TerminalTheme(v) => SettingsMutation::Set(Edit::Theme(v)),
            Self::ResetTerminalTheme => SettingsMutation::Remove(Key::Theme),
            Self::HistoryLimit(v) => SettingsMutation::Set(Edit::HistoryLimit(v)),
            Self::ResetHistoryLimit => SettingsMutation::Remove(Key::HistoryLimit),
            Self::TerminalPreset(value) => SettingsMutation::Set(Edit::ColorPreset(value)),
            Self::ResetTerminalPreset => SettingsMutation::Remove(Key::ColorPreset),
            Self::UiLanguage(value) => SettingsMutation::Set(Edit::UiLanguage(value)),
            Self::ResetUiLanguage => SettingsMutation::Remove(Key::UiLanguage),
            Self::UiAccent(value) => SettingsMutation::Set(Edit::UiAccent(value)),
            Self::ResetUiAccent => SettingsMutation::Remove(Key::UiAccent),
            Self::UiAppearance(value) => SettingsMutation::Set(Edit::UiAppearance(value)),
            Self::ResetUiAppearance => SettingsMutation::Remove(Key::UiAppearance),
            Self::FontSize(v) => SettingsMutation::Set(Edit::FontSize(v)),
            Self::LineHeightPercent(v) => SettingsMutation::Set(Edit::LineHeightPercent(v)),
            Self::LetterSpacing(v) => SettingsMutation::Set(Edit::LetterSpacing(v)),
            Self::FontFamily(v) => SettingsMutation::Set(Edit::FontFamily(v)),
            Self::FontFallback(v) => SettingsMutation::Set(Edit::FontFallback(v)),
            Self::FontLigatures(v) => SettingsMutation::Set(Edit::FontLigatures(v)),
            Self::ScrollOnInput(v) => SettingsMutation::Set(Edit::ScrollOnInput(v)),
            Self::ScrollOnOutput(v) => SettingsMutation::Set(Edit::ScrollOnOutput(v)),
            Self::ResetScrollOnInput => SettingsMutation::Remove(Key::ScrollOnInput),
            Self::ResetScrollOnOutput => SettingsMutation::Remove(Key::ScrollOnOutput),
            Self::MacOptionAsMeta(v) => SettingsMutation::Set(Edit::MacOptionAsMeta(v)),
            Self::ResetMacOptionAsMeta => SettingsMutation::Remove(Key::MacOptionAsMeta),
            Self::DeleteSend(v) => SettingsMutation::Set(Edit::DeleteSend(v)),
            Self::ResetDeleteSend => SettingsMutation::Remove(Key::DeleteSend),
            Self::TermType(v) => SettingsMutation::Set(Edit::TermType(v)),
            Self::ResetTermType => SettingsMutation::Remove(Key::TermType),
            Self::BackspaceSend(v) => SettingsMutation::Set(Edit::BackspaceSend(v)),
            Self::ResetBackspaceSend => SettingsMutation::Remove(Key::BackspaceSend),
            Self::CursorBlink(v) => SettingsMutation::Set(Edit::CursorBlink(v)),
            Self::CopyOnSelect(v) => SettingsMutation::Set(Edit::CopyOnSelect(v)),
            Self::RightClickPaste(v) => SettingsMutation::Set(Edit::RightClickPaste(v)),
            Self::ResetFontSize => SettingsMutation::Remove(Key::FontSize),
            Self::ResetLineHeightPercent => SettingsMutation::Remove(Key::LineHeightPercent),
            Self::ResetLetterSpacing => SettingsMutation::Remove(Key::LetterSpacing),
            Self::ResetFontFamily => SettingsMutation::Remove(Key::FontFamily),
            Self::ResetFontFallback => SettingsMutation::Remove(Key::FontFallback),
            Self::ResetFontLigatures => SettingsMutation::Remove(Key::FontLigatures),
            Self::ResetCursorBlink => SettingsMutation::Remove(Key::CursorBlink),
            Self::ResetCopyOnSelect => SettingsMutation::Remove(Key::CopyOnSelect),
            Self::ResetRightClickPaste => SettingsMutation::Remove(Key::RightClickPaste),
        }
    }
}
#[derive(serde::Serialize)]
pub(super) struct Projection {
    version: u16,
    token: Uuid,
    values: ResolvedSettings,
    terminal_presets: [pulse_store::settings::TerminalPresetPaint;
        pulse_store::settings::TerminalPreset::ALL.len()],
}
struct Capture {
    token: Uuid,
    page: Arc<PageLaunchScope>,
    snapshot: SettingsSnapshot,
}
struct Metadata {
    closed: bool,
    directory: Option<PathBuf>,
    capture: Option<Capture>,
}
pub(super) struct SettingsOwner {
    io: Arc<SettingsIo>,
    metadata: Mutex<Metadata>,
    credit: Arc<Semaphore>,
    idle: watch::Sender<bool>,
}
struct Credit {
    _shared: shared_io::IoCredit,
    permit: Option<OwnedSemaphorePermit>,
    idle: watch::Sender<bool>,
}
impl Drop for Credit {
    fn drop(&mut self) {
        drop(self.permit.take());
        self.idle.send_replace(true);
    }
}
impl SettingsOwner {
    #[cfg(test)]
    pub(super) fn new() -> Self {
        Self::with_io(Arc::new(SettingsIo::new()))
    }
    pub(super) fn with_io(io: Arc<SettingsIo>) -> Self {
        Self {
            io,
            metadata: Mutex::new(Metadata {
                closed: false,
                directory: None,
                capture: None,
            }),
            credit: Arc::new(Semaphore::new(1)),
            idle: watch::channel(true).0,
        }
    }
    fn check(&self, registry: &SessionRegistry, page: &PageLaunchScope) -> Result<()> {
        if self
            .metadata
            .lock()
            .map_err(|_| "workspace.settings_failed")?
            .closed
        {
            return Err("workspace.denied");
        }
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")
    }
    fn reserve(&self) -> Result<Credit> {
        let state = self
            .metadata
            .lock()
            .map_err(|_| "workspace.settings_failed")?;
        if state.closed {
            return Err("workspace.denied");
        }
        let permit = self
            .credit
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        let shared = self.io.reserve()?;
        self.idle.send_replace(false);
        Ok(Credit {
            _shared: shared,
            permit: Some(permit),
            idle: self.idle.clone(),
        })
    }
    fn location(&self, directory: &PathBuf) -> Result<()> {
        let mut state = self
            .metadata
            .lock()
            .map_err(|_| "workspace.settings_failed")?;
        if state.closed {
            return Err("workspace.denied");
        }
        if state.directory.as_ref().is_some_and(|old| old != directory) {
            return Err("workspace.denied");
        }
        self.io.location(directory)?;
        state.directory = Some(directory.clone());
        Ok(())
    }
    async fn io<T: Send + 'static>(
        self: &Arc<Self>,
        work: impl FnOnce(Arc<Self>) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let credit = self.reserve()?;
        let owner = self.clone();
        tokio::task::spawn_blocking(move || {
            let result = work(owner);
            drop(credit);
            result
        })
        .await
        .map_err(|_| "workspace.settings_failed")?
    }
    fn publish(
        &self,
        registry: &SessionRegistry,
        page: Arc<PageLaunchScope>,
        snapshot: SettingsSnapshot,
    ) -> Result<Projection> {
        let values = SettingsResolver::resolve(Some(snapshot.text()), None, None)
            .map_err(|_| "workspace.settings_invalid")?;
        self.check(registry, &page)?;
        let mut state = self
            .metadata
            .lock()
            .map_err(|_| "workspace.settings_failed")?;
        if state.closed {
            return Err("workspace.denied");
        }
        let token = Uuid::new_v4();
        state.capture = Some(Capture {
            token,
            page,
            snapshot,
        });
        Ok(Projection {
            version: 15,
            token,
            values,
            terminal_presets: pulse_store::settings::TerminalPreset::previews(),
        })
    }
    pub(super) async fn read(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
    ) -> Result<Projection> {
        self.check(&registry, &page)?;
        self.io(move |owner| {
            owner.check(&registry, &page)?;
            owner.location(&directory)?;
            let directory = pulse_store::state_directory::prepare(&directory)
                .map_err(|_| "workspace.settings_unavailable")?;
            owner.check(&registry, &page)?;
            let file = SettingsFile::open_private(directory)
                .map_err(|_| "workspace.settings_unavailable")?;
            let snapshot = file.load().map_err(|_| "workspace.settings_invalid")?;
            owner.publish(&registry, page, snapshot)
        })
        .await
    }
    #[cfg(all(unix, test))]
    pub(super) async fn terminal_preset(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
    ) -> Result<pulse_store::settings::TerminalPreset> {
        Ok(self
            .terminal_startup(registry, page, directory, false)
            .await?
            .preset)
    }
    pub(super) async fn resolved_terminal(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        system_dark: bool,
    ) -> Result<pulse_store::settings::ResolvedSettings> {
        self.check(&registry, &page)?;
        self.io(move |owner| {
            owner.check(&registry, &page)?;
            owner.location(&directory)?;
            let directory = pulse_store::state_directory::prepare(&directory)
                .map_err(|_| "workspace.settings_unavailable")?;
            let file = SettingsFile::open_private(directory)
                .map_err(|_| "workspace.settings_unavailable")?;
            let snapshot = file.load().map_err(|_| "workspace.settings_invalid")?;
            let mut resolved = SettingsResolver::resolve(Some(snapshot.text()), None, None)
                .map_err(|_| "workspace.settings_invalid")?;
            resolved.terminal_preset.value = resolved.terminal_startup(system_dark).preset;
            owner.check(&registry, &page)?;
            Ok(resolved)
        })
        .await
    }
    #[cfg(unix)]
    pub(super) async fn terminal_startup(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        system_dark: bool,
    ) -> Result<pulse_store::settings::TerminalStartup> {
        Ok(self
            .resolved_terminal(registry, page, directory, system_dark)
            .await?
            .terminal_startup(system_dark))
    }
    pub(super) async fn save(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        directory: PathBuf,
        token: Uuid,
        change: Change,
    ) -> Result<Projection> {
        self.check(&registry, &page)?;
        self.io(move |owner| {
            owner.check(&registry, &page)?;
            owner.location(&directory)?;
            let expected = {
                let mut state = owner
                    .metadata
                    .lock()
                    .map_err(|_| "workspace.settings_failed")?;
                let old = state.capture.as_ref().ok_or("workspace.settings_stale")?;
                if old.token != token || !Arc::ptr_eq(&old.page, &page) {
                    return Err("workspace.settings_stale");
                }
                state
                    .capture
                    .take()
                    .ok_or("workspace.settings_stale")?
                    .snapshot
            };
            let file = SettingsFile::open_private(directory)
                .map_err(|_| "workspace.settings_unavailable")?;
            let snapshot = file
                .save_authorized(&expected, change.mutation(), || {
                    owner.check(&registry, &page).is_ok()
                })
                .map_err(|_| "workspace.settings_save_unconfirmed")?;
            owner.publish(&registry, page, snapshot)
        })
        .await
    }
    pub(super) fn close(&self) {
        if let Ok(mut state) = self.metadata.lock() {
            state.closed = true;
            state.capture = None;
            self.credit.close();
        }
    }
    pub(super) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        self.close();
        let mut idle = self.idle.subscribe();
        while !*idle.borrow() {
            idle.changed()
                .await
                .map_err(|_| "workspace.settings_failed")?;
        }
        Ok(())
    }
}
#[tauri::command]
pub(super) async fn workspace_settings_read(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<Projection> {
    let (_, page) = current_document(
        &state.0.documents,
        state.0.owner,
        &window.url().map_err(|_| "workspace.denied")?,
        &nonce,
    )?;
    if !local_workspace(&window) {
        return Err("workspace.denied");
    }
    let directory = window
        .app_handle()
        .path()
        .app_config_dir()
        .map_err(|_| "workspace.settings_unavailable")?;
    let result = state
        .0
        .settings
        .read(state.0.registry.clone(), page, directory)
        .await?;
    eprintln!("workspace.settings_read_confirmed");
    Ok(result)
}
#[tauri::command]
pub(super) async fn workspace_settings_save(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
    change: Change,
) -> Result<Projection> {
    let (_, page) = current_document(
        &state.0.documents,
        state.0.owner,
        &window.url().map_err(|_| "workspace.denied")?,
        &nonce,
    )?;
    if !local_workspace(&window) {
        return Err("workspace.denied");
    }
    let token = Uuid::parse_str(&token).map_err(|_| "workspace.settings_stale")?;
    let directory = window
        .app_handle()
        .path()
        .app_config_dir()
        .map_err(|_| "workspace.settings_unavailable")?;
    let result = state
        .0
        .settings
        .save(state.0.registry.clone(), page, directory, token, change)
        .await?;
    eprintln!("workspace.settings_save_confirmed");
    Ok(result)
}

use crate::{
    Result, StoreError,
    settings::{ResolvedSettings, SettingsError, SettingsPatch, SettingsResolver},
    settings_edit::TerminalSettingEdit,
    settings_remove::TerminalSettingKey,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
/// Existing app_state schema, versioned namespaces; no host existence or authorization grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsScope {
    /// One group overlay, not recursively inherited ancestor groups.
    Group(Uuid),
    /// One host overlay, higher priority than a group.
    Host(Uuid),
}
impl SettingsScope {
    pub(crate) fn validate(self) -> Result<()> {
        if self.id().is_nil() {
            Err(StoreError::SettingsRecord)
        } else {
            Ok(())
        }
    }
    fn id(self) -> Uuid {
        match self {
            Self::Group(id) | Self::Host(id) => id,
        }
    }
    fn kind(self) -> Kind {
        match self {
            Self::Group(_) => Kind::Group,
            Self::Host(_) => Kind::Host,
        }
    }
    pub(crate) fn key(self) -> Result<String> {
        self.validate()?;
        let kind = match self {
            Self::Group(_) => "group",
            Self::Host(_) => "host",
        };
        Ok(format!("settings.{kind}.{}.v1", self.id()))
    }
}
/// Caller cannot forge a revision or transfer this snapshot to a different scope.
/// Not serializable or an IPC authorization payload; no command or secret fields.
///
/// ```compile_fail
/// use pulse_store::settings_scope::ScopedSettingsSnapshot;
/// fn forge(snapshot: &mut ScopedSettingsSnapshot) { snapshot.revision = 0; }
/// ```
/// ```compile_fail
/// use pulse_store::settings_scope::{ScopedSettingsSnapshot,SettingsScope};
/// fn move_scope(snapshot: &mut ScopedSettingsSnapshot, scope: SettingsScope) {
///     snapshot.scope = scope;
/// }
/// ```
/// ```compile_fail
/// use pulse_store::settings_scope::ScopedSettingsSnapshot;
/// fn send_to_webview(snapshot: &ScopedSettingsSnapshot) {
///     let _ = serde_json::to_string(snapshot);
/// }
/// ```
#[derive(Clone)]
pub struct ScopedSettingsSnapshot {
    pub(crate) scope: SettingsScope,
    pub(crate) revision: u64,
    pub(crate) patch: SettingsPatch,
}
impl ScopedSettingsSnapshot {
    /// Immutable namespace identity, not a permission.
    pub fn scope(&self) -> SettingsScope {
        self.scope
    }
    /// Revision zero means never persisted. Empty edited records retain their revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Validated immutable sparse model.
    pub fn patch(&self) -> &SettingsPatch {
        &self.patch
    }
    pub(crate) fn empty(scope: SettingsScope) -> Self {
        Self {
            scope,
            revision: 0,
            patch: SettingsPatch::default(),
        }
    }
    pub(crate) fn encode(&self) -> Result<String> {
        self.scope.validate()?;
        self.patch
            .validate()
            .map_err(|_| StoreError::SettingsRecord)?;
        if self.revision == 0 || self.revision > i64::MAX as u64 {
            return Err(StoreError::SettingsRecord);
        }
        if self.patch.ui != Default::default() || self.patch.terminal.theme.is_some() {
            return Err(StoreError::SettingsRecord);
        }
        let document = Document {
            schema_version: 1,
            kind: self.scope.kind(),
            id: self.scope.id().to_string(),
            revision: self.revision,
            settings: self.patch.clone(),
        };
        let text = serde_json::to_string(&document).map_err(|_| StoreError::SettingsRecord)?;
        if text.len() > RECORD_LIMIT {
            return Err(StoreError::SettingsRecord);
        }
        Ok(text)
    }
    pub(crate) fn decode(scope: SettingsScope, bytes: &[u8]) -> Result<Self> {
        scope.validate()?;
        if bytes.len() > RECORD_LIMIT {
            return Err(StoreError::SettingsRecord);
        }
        let document: Document =
            serde_json::from_slice(bytes).map_err(|_| StoreError::SettingsRecord)?;
        if document.schema_version != 1
            || document.settings.ui != Default::default()
            || document.settings.terminal.theme.is_some()
            || document.kind != scope.kind()
            || document.id != scope.id().to_string()
            || document.revision == 0
            || document.revision > i64::MAX as u64
        {
            return Err(StoreError::SettingsRecord);
        }
        document
            .settings
            .validate()
            .map_err(|_| StoreError::SettingsRecord)?;
        Ok(Self {
            scope,
            revision: document.revision,
            patch: document.settings,
        })
    }
}
pub(crate) const RECORD_LIMIT: usize = 8192;
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Group,
    Host,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema_version: u8,
    kind: Kind,
    id: String,
    revision: u64,
    settings: SettingsPatch,
}
/// Atomic edit of current durable data, not an unconditional cached replacement.
#[derive(Debug, Clone)]
pub enum ScopedSettingsEdit {
    /// Set one known scalar, including an explicit default/false override.
    Set(TerminalSettingEdit),
    /// Remove one known override, retaining other fields and the revision tombstone.
    Remove(TerminalSettingKey),
}
impl ScopedSettingsEdit {
    pub(crate) fn apply(self, patch: &mut SettingsPatch) -> Result<()> {
        match self {
            Self::Set(TerminalSettingEdit::HistoryLimit(value)) => {
                patch.terminal.history_limit = Some(value)
            }
            Self::Remove(TerminalSettingKey::HistoryLimit) => patch.terminal.history_limit = None,
            Self::Set(TerminalSettingEdit::UiAppearance(_))
            | Self::Set(TerminalSettingEdit::Theme(_))
            | Self::Remove(TerminalSettingKey::Theme)
            | Self::Remove(TerminalSettingKey::UiAppearance)
            | Self::Set(TerminalSettingEdit::UiAccent(_))
            | Self::Set(TerminalSettingEdit::UiLanguage(_))
            | Self::Remove(TerminalSettingKey::UiLanguage)
            | Self::Remove(TerminalSettingKey::UiAccent) => {
                return Err(StoreError::SettingsRecord);
            }
            Self::Set(TerminalSettingEdit::ColorPreset(value)) => {
                patch.terminal.color_preset = Some(value)
            }
            Self::Remove(TerminalSettingKey::ColorPreset) => patch.terminal.color_preset = None,
            Self::Set(TerminalSettingEdit::FontSize(value)) => {
                patch.terminal.font_size = Some(value)
            }
            Self::Set(TerminalSettingEdit::LineHeightPercent(value)) => {
                patch.terminal.line_height_percent = Some(value)
            }
            Self::Set(TerminalSettingEdit::LetterSpacing(value)) => {
                patch.terminal.letter_spacing = Some(value)
            }
            Self::Set(TerminalSettingEdit::FontFamily(value)) => {
                patch.terminal.font_family = Some(value)
            }
            Self::Set(TerminalSettingEdit::FontFallback(value)) => {
                patch.terminal.font_fallback = Some(value)
            }
            Self::Set(TerminalSettingEdit::FontLigatures(value)) => {
                patch.terminal.font_ligatures = Some(value)
            }
            Self::Set(TerminalSettingEdit::ScrollOnInput(value)) => {
                patch.terminal.scroll_on_input = Some(value)
            }
            Self::Set(TerminalSettingEdit::ScrollOnOutput(value)) => {
                patch.terminal.scroll_on_output = Some(value)
            }
            Self::Remove(TerminalSettingKey::ScrollOnInput) => {
                patch.terminal.scroll_on_input = None
            }
            Self::Remove(TerminalSettingKey::ScrollOnOutput) => {
                patch.terminal.scroll_on_output = None
            }
            Self::Set(TerminalSettingEdit::MacOptionAsMeta(value)) => {
                patch.terminal.mac_option_as_meta = Some(value)
            }
            Self::Remove(TerminalSettingKey::MacOptionAsMeta) => {
                patch.terminal.mac_option_as_meta = None
            }
            Self::Set(TerminalSettingEdit::DeleteSend(value)) => {
                patch.terminal.delete_send = Some(value)
            }
            Self::Remove(TerminalSettingKey::DeleteSend) => patch.terminal.delete_send = None,
            Self::Set(TerminalSettingEdit::TermType(value)) => {
                patch.terminal.term_type = Some(value)
            }
            Self::Remove(TerminalSettingKey::TermType) => patch.terminal.term_type = None,
            Self::Set(TerminalSettingEdit::BackspaceSend(value)) => {
                patch.terminal.backspace_send = Some(value)
            }
            Self::Remove(TerminalSettingKey::BackspaceSend) => patch.terminal.backspace_send = None,
            Self::Set(TerminalSettingEdit::CursorBlink(value)) => {
                patch.terminal.cursor_blink = Some(value)
            }
            Self::Set(TerminalSettingEdit::CopyOnSelect(value)) => {
                patch.terminal.copy_on_select = Some(value)
            }
            Self::Set(TerminalSettingEdit::RightClickPaste(value)) => {
                patch.terminal.right_click_paste = Some(value)
            }
            Self::Set(TerminalSettingEdit::ConfirmClose(value)) => {
                patch.terminal.confirm_close = Some(value)
            }
            Self::Remove(TerminalSettingKey::FontSize) => patch.terminal.font_size = None,
            Self::Remove(TerminalSettingKey::FontFamily) => patch.terminal.font_family = None,
            Self::Remove(TerminalSettingKey::FontFallback) => patch.terminal.font_fallback = None,
            Self::Remove(TerminalSettingKey::FontLigatures) => patch.terminal.font_ligatures = None,
            Self::Remove(TerminalSettingKey::LineHeightPercent) => {
                patch.terminal.line_height_percent = None
            }
            Self::Remove(TerminalSettingKey::LetterSpacing) => patch.terminal.letter_spacing = None,
            Self::Remove(TerminalSettingKey::CursorBlink) => patch.terminal.cursor_blink = None,
            Self::Remove(TerminalSettingKey::CopyOnSelect) => patch.terminal.copy_on_select = None,
            Self::Remove(TerminalSettingKey::RightClickPaste) => {
                patch.terminal.right_click_paste = None
            }
            Self::Remove(TerminalSettingKey::ConfirmClose) => patch.terminal.confirm_close = None,
        }
        patch.validate().map_err(|_| StoreError::SettingsRecord)
    }
}
/// Two optional namespaces read in one SQLite snapshot, never two independent reads.
pub struct SettingsLayers {
    pub(crate) group: Option<ScopedSettingsSnapshot>,
    pub(crate) host: Option<ScopedSettingsSnapshot>,
}
impl SettingsLayers {
    /// The requested group, or no group layer. Does not imply membership authorization.
    pub fn group(&self) -> Option<&ScopedSettingsSnapshot> {
        self.group.as_ref()
    }
    /// The requested host, or no host layer. Does not imply the host exists.
    pub fn host(&self) -> Option<&ScopedSettingsSnapshot> {
        self.host.as_ref()
    }
    /// Defaults < user < group < host, without parsing a reformatted database string.
    pub fn resolve(
        &self,
        user: Option<&SettingsPatch>,
    ) -> std::result::Result<ResolvedSettings, SettingsError> {
        SettingsResolver::resolve_patches(
            user,
            self.group.as_ref().map(|s| &s.patch),
            self.host.as_ref().map(|s| &s.patch),
        )
    }
}

//! Pure workspace UI keymap overlays. No filesystem, executable policy, or capability grant.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
/// Maximum source bytes admitted before TOML parsing.
pub const KEYMAP_TEXT_LIMIT: usize = 16_384;
const DEFAULT_WHEN: &str = "workspaceFocus && !editableFocus && !modalOpen";
const FIND_WHEN: &str =
    "workspaceFocus && terminalFocus && !editableFocus && !modalOpen && !broadcast";
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
/// Stable errors containing no source text, filesystem paths, or parser details.
pub enum KeymapError {
    /// Source exceeds the parsing budget.
    #[error("keymap.document_too_large")]
    Size,
    /// Malformed TOML, unknown fields, or invalid source types.
    #[error("keymap.invalid_document")]
    Document,
    /// Source version is not supported.
    #[error("keymap.unsupported_version")]
    Version,
    /// Chord, condition, duplicate action, or merged limit is invalid.
    #[error("keymap.invalid_binding")]
    Binding,
    /// A Shell key lacks explicit terminal exclusion.
    #[error("keymap.terminal_key_reserved")]
    TerminalKey,
    /// Another existing product route owns the gesture.
    #[error("keymap.product_key_reserved")]
    ProductKey,
    /// Exact or prefix sequences overlap in the same context.
    #[error("keymap.binding_conflict")]
    Conflict,
}
/// Only existing workspace UI actions, never native Capability IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeymapAction {
    /// Open Profile metadata management.
    Profiles,
    /// Open terminal settings.
    Settings,
    /// Open saved SSH host metadata.
    Hosts,
    /// Open saved identity metadata.
    Identities,
    /// Open Vault controls; not unlock authorization.
    Vault,
    /// Open saved SSH selection; not network authorization.
    Ssh,
    /// Open read-only history and search for the focused terminal.
    Find,
    /// Ask the existing controller to confirm closing its current session.
    CloseSession,
    /// Open the bounded command selector.
    Palette,
}
const ACTIONS: [KeymapAction; 9] = [
    KeymapAction::Profiles,
    KeymapAction::Settings,
    KeymapAction::Hosts,
    KeymapAction::Identities,
    KeymapAction::Vault,
    KeymapAction::Ssh,
    KeymapAction::Find,
    KeymapAction::CloseSession,
    KeymapAction::Palette,
];
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Sparse schema-versioned user overrides, independent of terminal settings.
pub struct KeymapPatch {
    /// Explicit version; only 1 is supported.
    pub schema_version: u16,
    #[serde(default)]
    /// At most nine distinct action overrides.
    pub bindings: Vec<KeymapOverride>,
}
/// One override per action. Empty keys disables shortcuts, not the action itself.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KeymapOverride {
    /// Existing UI selection ID, never a permission.
    pub action: KeymapAction,
    /// Up to four aliases; empty explicitly disables this action's shortcuts.
    pub keys: Vec<String>,
    #[serde(default)]
    /// Bounded conjunction of fixed boolean context fields, never executable code.
    pub when: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
/// Canonical effective binding for frontend validation.
pub struct KeymapBinding {
    /// Existing UI selection ID, never a permission.
    pub action: KeymapAction,
    /// Canonical modifiers and at most two chord steps.
    pub keys: String,
    /// Bounded conjunction of fixed boolean context fields, never executable code.
    pub when: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
/// Provenance of shortcut metadata, not authority.
pub enum KeymapSource {
    /// Built-in aliases, including no aliases for unbound actions.
    Default,
    /// Explicit user override.
    User,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
/// Provenance and shortcut availability for one of the nine UI actions.
pub struct KeymapActionSource {
    /// Existing UI selection ID, never a permission.
    pub action: KeymapAction,
    /// Built-in or explicitly overridden source.
    pub source: KeymapSource,
    /// No effective shortcut; the UI action and its backend permission are unaffected.
    pub disabled: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
/// Bounded frontend selection metadata with no execution handlers.
pub struct KeymapProjection {
    /// Projection version, currently 1.
    pub version: u16,
    /// At most sixteen validated effective aliases.
    pub bindings: Vec<KeymapBinding>,
    /// Exactly nine action sources, including explicitly disabled overrides.
    pub sources: Vec<KeymapActionSource>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Chord {
    modifiers: [bool; 4],
    key: String,
}
impl Chord {
    fn parse(text: &str) -> Result<Self, KeymapError> {
        let mut parts: Vec<_> = text.split('+').collect();
        let key = parts.pop().ok_or(KeymapError::Binding)?;
        let valid = (key.len() == 1
            && key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
            || matches!(
                key,
                "enter"
                    | "escape"
                    | "tab"
                    | "space"
                    | "backspace"
                    | "delete"
                    | "insert"
                    | "home"
                    | "end"
                    | "pageup"
                    | "pagedown"
                    | "arrowleft"
                    | "arrowright"
                    | "arrowup"
                    | "arrowdown"
            )
            || (1..=12).any(|number| key == format!("f{number}"));
        if !valid || parts.len() > 4 {
            return Err(KeymapError::Binding);
        }
        let mut modifiers = [false; 4];
        for part in parts {
            let index = ["ctrl", "meta", "alt", "shift"]
                .iter()
                .position(|name| *name == part)
                .ok_or(KeymapError::Binding)?;
            if modifiers[index] {
                return Err(KeymapError::Binding);
            }
            modifiers[index] = true;
        }
        Ok(Self {
            modifiers,
            key: key.into(),
        })
    }
    fn canonical(&self) -> String {
        ["ctrl", "meta", "alt", "shift"]
            .into_iter()
            .zip(self.modifiers)
            .filter_map(|(name, used)| used.then_some(name))
            .chain(std::iter::once(self.key.as_str()))
            .collect::<Vec<_>>()
            .join("+")
    }
    fn app_candidate(&self) -> bool {
        !self.modifiers[2]
            && self.modifiers[3]
            && (self.modifiers[0] != self.modifiers[1])
            && self.key.len() == 1
    }
}
fn sequence(text: &str) -> Result<Vec<Chord>, KeymapError> {
    if text.is_empty() || text.len() > 128 || !text.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return Err(KeymapError::Binding);
    }
    let lower = text.to_ascii_lowercase();
    let parts: Vec<_> = lower.split(' ').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() || parts.len() > 2 {
        return Err(KeymapError::Binding);
    }
    parts.into_iter().map(Chord::parse).collect()
}
fn condition(text: &str) -> Result<Vec<(usize, bool)>, KeymapError> {
    if text.len() > 128 {
        return Err(KeymapError::Binding);
    }
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut terms = Vec::new();
    for part in text.split("&&") {
        let part = part.trim();
        let (name, required) = part
            .strip_prefix('!')
            .map_or((part, true), |name| (name, false));
        let field = [
            "workspaceFocus",
            "terminalFocus",
            "editableFocus",
            "modalOpen",
            "broadcast",
        ]
        .iter()
        .position(|field| *field == name)
        .ok_or(KeymapError::Binding)?;
        if terms.len() >= 5 || terms.iter().any(|(existing, _)| *existing == field) {
            return Err(KeymapError::Binding);
        }
        terms.push((field, required));
    }
    Ok(terms)
}
type ParsedBinding = (Vec<Chord>, Vec<(usize, bool)>);
fn validate_binding(binding: &KeymapBinding) -> Result<ParsedBinding, KeymapError> {
    let sequence = sequence(&binding.keys)?;
    let when = condition(&binding.when)?;
    if !when.contains(&(1, false)) && sequence.iter().any(|chord| !chord.app_candidate()) {
        return Err(KeymapError::TerminalKey);
    }
    for chord in &sequence {
        if (chord.modifiers[0] || chord.modifiers[1])
            && chord.modifiers[3]
            && !chord.modifiers[2]
            && (matches!(chord.key.as_str(), "c" | "v" | "r")
                || (chord.key == "p" && binding.action != KeymapAction::Palette))
        {
            return Err(KeymapError::ProductKey);
        }
    }
    Ok((sequence, when))
}
impl KeymapPatch {
    /// Validate and encode a bounded canonical document without granting file or execution access.
    pub fn to_toml(&self) -> Result<String, KeymapError> {
        self.resolve()?;
        let text = toml::to_string(self).map_err(|_| KeymapError::Document)?;
        if text.len() > KEYMAP_TEXT_LIMIT {
            return Err(KeymapError::Size);
        }
        Ok(text)
    }
    /// Parse and resolve the complete bounded source, rejecting invalid merges.
    pub fn parse(text: &str) -> Result<Self, KeymapError> {
        if text.len() > KEYMAP_TEXT_LIMIT {
            return Err(KeymapError::Size);
        }
        let patch: Self = toml::from_str(text).map_err(|_| KeymapError::Document)?;
        patch.resolve()?;
        Ok(patch)
    }
    /// Validate directly constructed patches too. Missing actions inherit; empty aliases disable.
    pub fn resolve(&self) -> Result<KeymapProjection, KeymapError> {
        if self.schema_version != 1 {
            return Err(KeymapError::Version);
        }
        if self.bindings.len() > ACTIONS.len() {
            return Err(KeymapError::Binding);
        }
        let mut seen = HashSet::new();
        for entry in &self.bindings {
            if !seen.insert(entry.action) || entry.keys.len() > 4 {
                return Err(KeymapError::Binding);
            }
            condition(&entry.when)?;
            for key in &entry.keys {
                validate_binding(&KeymapBinding {
                    action: entry.action,
                    keys: key.clone(),
                    when: entry.when.clone(),
                })?;
            }
        }
        let mut bindings = Vec::new();
        let mut sources = Vec::new();
        for action in ACTIONS {
            if let Some(entry) = self.bindings.iter().find(|entry| entry.action == action) {
                for keys in &entry.keys {
                    bindings.push(KeymapBinding {
                        action,
                        keys: sequence(keys)?
                            .iter()
                            .map(Chord::canonical)
                            .collect::<Vec<_>>()
                            .join(" "),
                        when: entry.when.clone(),
                    });
                }
                sources.push(KeymapActionSource {
                    action,
                    source: KeymapSource::User,
                    disabled: entry.keys.is_empty(),
                });
            } else {
                let (keys, when): (&[&str], &str) = match action {
                    KeymapAction::Palette => (&["ctrl+shift+p", "meta+shift+p"], DEFAULT_WHEN),
                    KeymapAction::Find => (&["ctrl+shift+f", "meta+shift+f"], FIND_WHEN),
                    _ => (&[], ""),
                };
                for key in keys {
                    bindings.push(KeymapBinding {
                        action,
                        keys: (*key).into(),
                        when: when.into(),
                    });
                }
                sources.push(KeymapActionSource {
                    action,
                    source: KeymapSource::Default,
                    disabled: !matches!(action, KeymapAction::Palette | KeymapAction::Find),
                });
            }
        }
        if bindings.len() > 16 {
            return Err(KeymapError::Binding);
        }
        let validated = bindings
            .iter()
            .map(validate_binding)
            .collect::<Result<Vec<_>, _>>()?;
        for (index, (left, left_when)) in validated.iter().enumerate() {
            for (right, right_when) in validated.iter().skip(index + 1) {
                let overlap = !left_when
                    .iter()
                    .any(|(field, required)| right_when.contains(&(*field, !required)));
                if overlap && left.iter().zip(right).all(|(left, right)| left == right) {
                    return Err(KeymapError::Conflict);
                }
            }
        }
        Ok(KeymapProjection {
            version: 1,
            bindings,
            sources,
        })
    }
    /// Per-action restoration is override removal, never a rewrite of compiled defaults.
    pub fn restore_action(&mut self, action: KeymapAction) {
        self.bindings.retain(|entry| entry.action != action);
    }
}
#[cfg(test)]
mod tests;

use crate::{Result, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
/// Stable automatic Unix shell identity; never an executable filename or WSL name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomaticShell {
    /// Bash automatic profile.
    Bash,
    /// Zsh automatic profile.
    Zsh,
    /// Fish automatic profile.
    Fish,
    /// Generic sh automatic profile.
    Sh,
    /// PowerShell discovered on Unix.
    Pwsh,
}
/// Versioned non-secret preferences. Invalid import never mutates a live catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoProfilePreferences {
    hidden: BTreeSet<AutomaticShell>,
    deleted: BTreeSet<AutomaticShell>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema_version: u8,
    hidden: Vec<AutomaticShell>,
    deleted: Vec<AutomaticShell>,
}
impl AutoProfilePreferences {
    /// Hidden identities, including currently undiscovered profiles.
    pub fn hidden(&self) -> impl Iterator<Item = AutomaticShell> + '_ {
        self.hidden.iter().copied()
    }
    /// Deleted identities; later discovery must not recreate them.
    pub fn deleted(&self) -> impl Iterator<Item = AutomaticShell> + '_ {
        self.deleted.iter().copied()
    }
    /// Change visibility without authorizing a launch or reviving a deleted profile.
    pub fn set_hidden(&mut self, shell: AutomaticShell, hidden: bool) -> Result<()> {
        if self.deleted.contains(&shell) {
            return Err(StoreError::Record);
        }
        if hidden {
            self.hidden.insert(shell);
        } else {
            self.hidden.remove(&shell);
        }
        Ok(())
    }
    /// Remove a visibility override and keep deletion suppression across restarts.
    pub fn suppress(&mut self, shell: AutomaticShell) {
        self.hidden.remove(&shell);
        self.deleted.insert(shell);
    }
    pub(crate) fn encode(&self) -> Result<String> {
        serde_json::to_string(&Document {
            schema_version: 1,
            hidden: self.hidden().collect(),
            deleted: self.deleted().collect(),
        })
        .map_err(|_| StoreError::Record)
    }
    pub(crate) fn decode(text: &str) -> Result<Self> {
        if text.len() > 4096 {
            return Err(StoreError::Record);
        }
        let d: Document = serde_json::from_str(text).map_err(|_| StoreError::Record)?;
        if d.schema_version != 1 || d.hidden.len() > 5 || d.deleted.len() > 5 {
            return Err(StoreError::Record);
        }
        let hidden: BTreeSet<_> = d.hidden.iter().copied().collect();
        let deleted: BTreeSet<_> = d.deleted.iter().copied().collect();
        if hidden.len() != d.hidden.len()
            || deleted.len() != d.deleted.len()
            || !hidden.is_disjoint(&deleted)
        {
            return Err(StoreError::Record);
        }
        Ok(Self { hidden, deleted })
    }
}

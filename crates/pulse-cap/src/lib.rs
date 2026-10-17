//! Shared bounded action metadata and typed argument validation.
//! This catalogue is not a caller authenticator, target permission, confirmation, or executor.
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::BTreeMap;
/// Maximum registered actions in the current catalogue.
pub const MAX_ACTIONS: usize = 64;
/// Maximum serialized single argument schema.
pub const MAX_SCHEMA_BYTES: usize = 16 * 1024;
/// Metadata publication fits the existing CLI message-body budget.
pub const MAX_CATALOGUE_BYTES: usize = 64 * 1024;
/// Parameters must fit a bounded control message before typed deserialization.
pub const MAX_ARGS_BYTES: usize = 64 * 1024;
/// Entry point selected by a trusted transport adapter, not by client parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    /// Existing command selector.
    Palette,
    /// Existing keyboard gestures.
    Keybinding,
    /// Authenticated local CLI; still requires independent action/target admission.
    Cli,
    /// Explicitly allowed MCP tools.
    Mcp,
    /// Explicitly allowed plugin actions.
    Plugin,
}
/// Required backend target category; IDs are resolved and authorized separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Application-owned global operation.
    Global,
    /// Backend-authorized current window.
    Window,
    /// Backend-authorized session, not possession of a session identifier.
    Session,
}
/// Non-safe external requests require separate current-GUI confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// No risk dialog, but caller/surface/target permission is still required.
    Safe,
    /// Explicit original-request GUI confirmation.
    Confirm,
    /// Elevated risk and explicit original-request GUI confirmation.
    Dangerous,
}
/// Content-free catalogue failures; no parameter or parser text is exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Invalid static ID/localization key, empty/duplicate surfaces or duplicate action.
    #[error("cap.invalid_definition")]
    Definition,
    /// Schema, catalogue or parameter bytes exceed the bounded control budget.
    #[error("cap.budget")]
    Budget,
    /// Unknown action or unavailable entry surface.
    #[error("cap.unavailable")]
    Unavailable,
    /// Wrong parameter shape, unknown fields or semantic bounds.
    #[error("cap.invalid_args")]
    Args,
}
/// Trusted Rust registration metadata. No handler or permission is contained here.
pub struct ActionDef {
    /// Stable public action identifier; published IDs must not be renamed.
    pub id: &'static str,
    /// Localization key, never caller-supplied display copy.
    pub title: &'static str,
    /// Required target category.
    pub scope: Scope,
    /// Risk policy advertised to adapters, not a satisfied confirmation.
    pub risk: Risk,
    /// Explicit allowed entry points.
    pub surfaces: &'static [Surface],
}
/// Registered argument types generate their schema and validate semantic bounds together.
/// Implementations should reject unknown fields using serde's `deny_unknown_fields`.
pub trait ActionArgs: DeserializeOwned + JsonSchema {
    /// Validate bounds that cannot be represented by the Rust field type alone.
    fn validate(&self) -> Result<(), Error>;
}
#[derive(Serialize)]
struct Metadata {
    id: &'static str,
    title: &'static str,
    scope: Scope,
    risk: Risk,
    surfaces: Vec<Surface>,
    args: Value,
}
struct Entry {
    metadata: Metadata,
    validate: fn(Value) -> Result<(), Error>,
}
/// Trusted registration builder. Duplicate registration never replaces the original entry.
#[derive(Default)]
pub struct Builder {
    entries: BTreeMap<&'static str, Entry>,
}
fn identifier(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}
fn check_argument_bytes(value: &Value) -> Result<(), Error> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_ARGS_BYTES - self.0 {
                return Err(std::io::Error::other("cap.budget"));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    // Count into a discard writer, never allocate another plaintext parameter buffer.
    serde_json::to_writer(Counter(0), value).map_err(|_| Error::Budget)
}

fn closed_objects(schema: &Value) -> bool {
    match schema {
        Value::Object(object) => {
            let object_type = object.get("type").is_some_and(|kind| {
                kind == "object"
                    || kind
                        .as_array()
                        .is_some_and(|types| types.iter().any(|kind| kind == "object"))
            });
            if object_type && object.get("additionalProperties") != Some(&Value::Bool(false)) {
                return false;
            }
            object.values().all(closed_objects)
        }
        Value::Array(items) => items.iter().all(closed_objects),
        _ => true,
    }
}

fn validate_typed<A: ActionArgs>(value: Value) -> Result<(), Error> {
    let args: A = serde_json::from_value(value).map_err(|_| Error::Args)?;
    args.validate()
}
impl Builder {
    /// Register a static action and its generated typed schema; no effect handler is enabled.
    pub fn register<A: ActionArgs>(&mut self, definition: ActionDef) -> Result<(), Error> {
        if !identifier(definition.id, 96)
            || !identifier(definition.title, 128)
            || definition.surfaces.is_empty()
            || definition.surfaces.len() > 5
            || self.entries.contains_key(definition.id)
        {
            return Err(Error::Definition);
        }
        let mut surfaces = definition.surfaces.to_vec();
        surfaces.sort_unstable();
        surfaces.dedup();
        if surfaces.len() != definition.surfaces.len() {
            return Err(Error::Definition);
        }
        if self.entries.len() >= MAX_ACTIONS {
            return Err(Error::Budget);
        }
        let schema =
            serde_json::to_value(schemars::schema_for!(A)).map_err(|_| Error::Definition)?;
        // Current adapters only admit closed object schemas. This avoids silent unknown-field loss.
        if schema.get("type").and_then(Value::as_str) != Some("object") || !closed_objects(&schema)
        {
            return Err(Error::Definition);
        }
        if serde_json::to_vec(&schema)
            .map_err(|_| Error::Definition)?
            .len()
            > MAX_SCHEMA_BYTES
        {
            return Err(Error::Budget);
        }
        self.entries.insert(
            definition.id,
            Entry {
                metadata: Metadata {
                    id: definition.id,
                    title: definition.title,
                    scope: definition.scope,
                    risk: definition.risk,
                    surfaces,
                    args: schema,
                },
                validate: validate_typed::<A>,
            },
        );
        Ok(())
    }
    /// Freeze the catalogue after checking publication budgets. Not an execution registration.
    pub fn finish(self) -> Result<Catalogue, Error> {
        let catalogue = Catalogue {
            entries: self.entries,
        };
        for surface in [
            Surface::Palette,
            Surface::Keybinding,
            Surface::Cli,
            Surface::Mcp,
            Surface::Plugin,
        ] {
            catalogue.projection(surface)?;
        }
        Ok(catalogue)
    }
}
/// Immutable shared metadata/argument catalogue. It cannot dispatch effects or grant access.
pub struct Catalogue {
    entries: BTreeMap<&'static str, Entry>,
}
impl Catalogue {
    /// Read-only target/risk policy for a trusted entry adapter, never an approval.
    pub fn policy(&self, id: &str, surface: Surface) -> Result<(Scope, Risk), Error> {
        let entry = self
            .entries
            .get(id)
            .filter(|entry| entry.metadata.surfaces.contains(&surface))
            .ok_or(Error::Unavailable)?;
        Ok((entry.metadata.scope, entry.metadata.risk))
    }
    /// Return a bounded per-surface list, without executable handlers or permission receipts.
    pub fn projection(&self, surface: Surface) -> Result<Value, Error> {
        let actions: Vec<_> = self
            .entries
            .values()
            .filter(|entry| entry.metadata.surfaces.contains(&surface))
            .map(|entry| &entry.metadata)
            .collect();
        let value = serde_json::json!({"version":1,"actions":actions});
        if serde_json::to_vec(&value)
            .map_err(|_| Error::Definition)?
            .len()
            > MAX_CATALOGUE_BYTES
        {
            return Err(Error::Budget);
        }
        Ok(value)
    }
    /// Validate only action visibility and arguments. Success is expressly not invocation permission.
    /// A trusted adapter must still verify original caller, current target and GUI risk confirmation.
    pub fn validate_args(&self, id: &str, surface: Surface, value: Value) -> Result<(), Error> {
        self.checked_args(id, surface, value).map(|_| ())
    }
    /// Return original bounded arguments after typed checks, not an execution receipt.
    /// Any typed-deserialization copy is made only after the control-message budget is verified.
    pub fn checked_args(&self, id: &str, surface: Surface, value: Value) -> Result<Value, Error> {
        let entry = self
            .entries
            .get(id)
            .filter(|entry| entry.metadata.surfaces.contains(&surface))
            .ok_or(Error::Unavailable)?;
        if !value.is_object() {
            return Err(Error::Args);
        }
        check_argument_bytes(&value)?;
        (entry.validate)(value.clone())?;
        Ok(value)
    }
}
#[cfg(test)]
mod tests;

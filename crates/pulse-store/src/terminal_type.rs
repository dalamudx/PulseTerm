//! Supported TERM advertisements; this does not change the parser or grant startup authority.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Fixed terminal type advertised to a new local or SSH session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum TerminalType {
    /// The existing 256-color default.
    #[default]
    #[serde(rename = "xterm-256color")]
    Xterm256color,
    /// Advertise the xterm capability name.
    #[serde(rename = "xterm")]
    Xterm,
    /// Advertise the VT100 capability name without changing parser emulation.
    #[serde(rename = "vt100")]
    Vt100,
}
impl TerminalType {
    /// Literal TERM value, never an executable or shell expression.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Xterm256color => "xterm-256color",
            Self::Xterm => "xterm",
            Self::Vt100 => "vt100",
        }
    }
}

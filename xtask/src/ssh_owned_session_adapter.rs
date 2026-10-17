//! Validation compiles the SAME upper-layer adapter as the opt-in production workspace.
//! Fixed server, password and directory fixtures remain solely in the probe.
pub(super) use crate::ssh_session_adapter::SessionSink;
pub(super) use crate::ssh_session_adapter::requests::{SessionRequests, StartTarget};

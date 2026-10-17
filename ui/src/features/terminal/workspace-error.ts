/** Fixed content-free diagnostic code; no raw native error or terminal content. */
export function workspaceTerminalError(error: Error): string {
  const allowed = [
    "input.uncertain",
    "input.transport",
    "view.retired",
    "terminal.dom_projection",
    "terminal.dom_geometry",
    "terminal.dom_rows",
    "terminal.render_timeout",
    "terminal.render_cancelled",
    "terminal.render_unavailable",
    "terminal.style_nonce_missing",
    "terminal.connection_failed",
    "terminal.write_failed",
    "terminal.font_unavailable",
    "terminal.font_timeout",

    "terminal.write_busy",
    "terminal.surface_disposed",
    "terminal.write_oversize",
    "attach2.consume",
    "attach2.queue_limit",
    "attach2.cancelled",
    "attach.sequence",
    "attach.phase",
    "attach.data",
    "attach.checkpoint",
    "attach.checkpoint_revision",
    "attach.generation",
    "attach.failed",
    "attach.length",
    "attach.kind",
    "attach.limits",
    "attach.part",
    "attach.restore",
    "attach.version_or_flags",
    "attach2.failed",
    "attach2.length_or_kind",
    "attach2.source_regression",
    "attach2.source_boundary",
    "attach2.queue_limits",
    "terminal.not_visible",
    "terminal.bootstrap_timeout",
    "terminal.non_raw_channel",
    "terminal.consume_failed",
    "workspace.denied",
  ];
  const message = error.message.startsWith("terminal.render_timeout.frames=")
    ? "terminal.render_timeout"
    : error.message;
  const code = allowed.includes(message) ? message : "unknown";
  return code;
}

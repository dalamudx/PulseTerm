import { message, type UiMessage } from "../../workspace-language";
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

/** User guidance is selected from fixed codes; native and remote text is never interpolated. */
export function workspaceTerminalFailure(
  error: unknown,
  fallback: string | UiMessage,
): UiMessage {
  const otherwise = typeof fallback === "string" ? message(fallback) : fallback;
  if (!(error instanceof Error)) return otherwise;
  switch (error.message) {
    case "input.uncertain":
    case "input.transport":
    case "input.overloaded":
      return message(
        "输入结果未确认 · 请先检查终端状态，不要重复发送刚才的输入。",
      );
    case "workspace.visibility_timeout":
    case "workspace.visibility_failed":
      return message("无法确认窗口显示状态 · 已暂停输入，请重新加载工作区。");
  }
  switch (workspaceTerminalError(error)) {
    case "terminal.font_unavailable":
    case "terminal.font_timeout":
      return message(
        "终端字体未就绪 · 请在设置中选择可用字体，再重新选择会话。",
      );
    case "terminal.dom_projection":
    case "terminal.dom_geometry":
    case "terminal.dom_rows":
      return message("终端显示尺寸无法确认 · 请调整窗口大小后重新选择会话。");
    case "terminal.bootstrap_timeout":
      return message("恢复终端显示超时 · 会话仍保留，请重新选择会话。");
    case "terminal.render_timeout":
    case "terminal.render_unavailable":
    case "terminal.write_failed":
      return message("终端绘制已中断 · 会话仍保留，请重新选择会话。");
    case "terminal.write_oversize":
    case "attach2.queue_limit":
    case "attach2.queue_limits":
    case "attach.limits":
      return message("终端输出处理已超出上限 · 请重新选择会话恢复显示。");
    default:
      return otherwise;
  }
}

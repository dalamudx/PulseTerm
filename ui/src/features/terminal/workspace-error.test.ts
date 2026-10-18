import { expect, it } from "vitest";
import { message, renderUiMessage } from "../../workspace-language";
import {
  workspaceTerminalError,
  workspaceTerminalFailure,
} from "./workspace-error";

const fallback = message("无法恢复显示 · 会话仍保留，可重新选择");

it("keeps uncertain or interrupted input distinct from retryable display recovery", () => {
  for (const code of [
    "input.uncertain",
    "input.transport",
    "input.overloaded",
  ]) {
    const notice = workspaceTerminalFailure(Error(code), fallback);
    expect(renderUiMessage("zh-CN", notice)).toContain(
      "不要重复发送刚才的输入",
    );
    expect(renderUiMessage("en", notice)).toContain(
      "do not resend the last input",
    );
  }
  const notice = workspaceTerminalFailure(
    Error("terminal.render_timeout.frames=120 last=16"),
    fallback,
  );
  expect(renderUiMessage("en", notice)).toContain(
    "Terminal rendering interrupted",
  );
  expect(
    workspaceTerminalError(Error("terminal.render_timeout.frames=120 last=16")),
  ).toBe("terminal.render_timeout");
});

it("never displays native/remote content, even when it resembles a known error", () => {
  for (const raw of [
    "input.uncertain password=secret",
    "terminal.font_unavailable /private/keys/id_rsa",
    "<img src=x onerror=alert(1)>",
    "\u001b]52;c;secret\u0007",
    "workspace.denied secret",
  ]) {
    expect(workspaceTerminalError(Error(raw))).toBe("unknown");
    expect(workspaceTerminalFailure(Error(raw), fallback)).toBe(fallback);
  }
  for (const value of [null, "input.uncertain", { message: "input.uncertain" }])
    expect(workspaceTerminalFailure(value, fallback)).toBe(fallback);
});

it("preserves stage-specific fallback and offers concrete recovery for known failures in both languages", () => {
  expect(workspaceTerminalFailure(Error("workspace.denied"), fallback)).toBe(
    fallback,
  );
  const stages = [
    ["terminal.font_timeout", "选择可用字体", "Choose an available font"],
    ["terminal.dom_geometry", "调整窗口大小", "Resize the window"],
    ["workspace.visibility_failed", "重新加载工作区", "reload the workspace"],
    ["terminal.bootstrap_timeout", "重新选择会话", "select it again"],
    ["attach2.queue_limits", "超出上限", "processing limit"],
  ];
  for (const [code, chinese, english] of stages) {
    const notice = workspaceTerminalFailure(Error(code), fallback);
    expect(renderUiMessage("zh-CN", notice)).toContain(chinese);
    expect(renderUiMessage("en", notice)).toContain(english);
    expect(renderUiMessage("en", notice)).not.toMatch(
      /(?:input|terminal|attach2?|workspace)[.][a-z_]+/,
    );
  }
});

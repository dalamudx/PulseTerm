import { workspaceDecision } from "./workspace-decision";
import { message } from "./workspace-language";
import { decodeWorkspaceSessions } from "./workspace-sessions";
export function confirmWorkspaceClose(
  doc: Document,
  session: string,
  signal: AbortSignal,
): Promise<boolean> {
  decodeWorkspaceSessions([session]);
  return workspaceDecision(
    doc,
    {
      title: message("关闭会话 {0}？", session),
      description:
        "会话中可能仍有程序运行。关闭会终止会话连接并回收本地原生资源，不是仅隐藏标签。",
      accept: "关闭会话",
      cancel: "保留会话",
    },
    signal,
  );
}
export async function closeWorkspaceSession(
  session: string,
  signal: AbortSignal,
  current: () => boolean,
  confirm: () => Promise<boolean>,
  close: () => Promise<void>,
): Promise<"cancelled" | "closed"> {
  decodeWorkspaceSessions([session]);
  if (signal.aborted || !current()) return "cancelled";
  if (!(await confirm()) || signal.aborted || !current()) return "cancelled";
  await close();
  return "closed";
}

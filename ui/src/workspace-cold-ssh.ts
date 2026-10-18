import type { CliSshIntent } from "./workspace-cli-new-tab";
import { workspaceDecision } from "./workspace-decision";
import { message } from "./workspace-language";
import type { VaultStatus } from "./workspace-vault";

interface Dependencies {
  read(signal: AbortSignal): Promise<VaultStatus>;
  unlock(signal: AbortSignal): Promise<void>;
}
export async function confirmColdSsh(
  document: Document,
  selection: CliSshIntent,
  signal: AbortSignal,
  dependencies: Dependencies,
): Promise<boolean> {
  const decide = (unlocked: boolean) =>
    workspaceDecision(
      document,
      {
        title: "查看 CLI 请求的 SSH 目标？",
        description: unlocked
          ? "查看保存的主机和身份后，仍需确认连接目标及实际指纹。"
          : "先解锁保险库，关闭保险库窗口后再次点击查看目标。解锁不会连接主机。",
        preview:
          selection.alias !== undefined
            ? message("保存的 SSH 入口：{0}", selection.alias)
            : selection.authentication === "password-interactive"
              ? message(
                  "主机 {0}\n身份 {1}\n认证方式：密码 + MFA（两种因素均须成功）",
                  selection.hostId,
                  selection.identityId,
                )
              : selection.authentication === "agent-interactive"
                ? message(
                    "主机 {0}\n身份 {1}\n认证方式：Agent + MFA（两种因素均须成功）",
                    selection.hostId,
                    selection.identityId,
                  )
                : selection.authentication === "key-interactive"
                  ? message(
                      "主机 {0}\n身份 {1}\n认证方式：私钥 + MFA（两种因素均须成功）",
                      selection.hostId,
                      selection.identityId,
                    )
                  : selection.authentication === "interactive"
                    ? message(
                        "主机 {0}\n身份 {1}\n认证方式：交互认证 / MFA",
                        selection.hostId,
                        selection.identityId,
                      )
                    : message(
                        "主机 {0}\n身份 {1}",
                        selection.hostId,
                        selection.identityId,
                      ),
        previewLabel: "请求的保存记录",
        accept: unlocked ? "查看目标" : "解锁保险库",
        cancel: "取消",
      },
      signal,
    );
  if (signal.aborted) return false;
  const initial = await dependencies.read(signal);
  if (signal.aborted || !(await decide(initial.state === "available")))
    return false;
  if (initial.state === "available") return !signal.aborted;
  await dependencies.unlock(signal);
  if (signal.aborted) return false;
  const current = await dependencies.read(signal);
  if (signal.aborted || current.state !== "available") return false;
  // Unlock completion never substitutes for this separate confirmation.
  return (await decide(true)) && !signal.aborted;
}

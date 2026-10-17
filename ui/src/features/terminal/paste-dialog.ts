import { workspaceDecision } from "../../workspace-decision";
import { message } from "../../workspace-language";
import type { PasteAssessment } from "./paste-policy";

function visiblePreview(text: string): string {
  return [...text]
    .map((character) => {
      const code = character.codePointAt(0) ?? 0;
      if (character === "\r") return "\\r";
      if (character === "\t") return "\\t";
      if (
        (code >= 0x200b && code <= 0x200f) ||
        code === 0x061c ||
        (code >= 0x2028 && code <= 0x202e) ||
        (code >= 0x2060 && code <= 0x206f) ||
        code === 0xfeff
      )
        return `\\u{${code.toString(16)}}`;
      return character;
    })
    .join("");
}
export function confirmTerminalPaste(
  doc: Document,
  text: string,
  assessment: PasteAssessment,
  signal: AbortSignal,
): Promise<boolean> {
  return workspaceDecision(
    doc,
    {
      title: "确认终端粘贴",
      description: message(
        "{0} 行，{1} UTF-8 字节。换行可能执行命令；Tab 和隐形字符可能改变含义。",
        assessment.lines,
        assessment.bytes,
      ),
      preview: visiblePreview(text),
      previewLabel: "粘贴内容预览（隐形字符转义显示）",
      accept: "粘贴一次",
      cancel: "取消",
    },
    signal,
  );
}

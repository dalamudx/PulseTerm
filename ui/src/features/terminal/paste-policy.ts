export const MAX_PASTE_BYTES = 1012; // Leave 12 bytes in the existing 1024-byte message for bracket framing.
export type PasteAssessment = Readonly<{
  bytes: number;
  lines: number;
  confirmation: boolean;
}>;
export function assessPaste(text: string): PasteAssessment {
  if (!text || text.length > MAX_PASTE_BYTES) throw Error("paste.size");
  const bytes = new TextEncoder().encode(text);
  if (bytes.length > MAX_PASTE_BYTES) throw Error("paste.size");
  if (new TextDecoder("utf-8", { ignoreBOM: true }).decode(bytes) !== text)
    throw Error("paste.invalid_unicode");
  // Reject rather than silently transform escape/NUL/C1 into a different command.
  if (
    [...text].some((character) => {
      const code = character.codePointAt(0) ?? 0;
      return (
        (code < 32 && code !== 9 && code !== 10 && code !== 13) ||
        (code >= 127 && code <= 159)
      );
    })
  )
    throw Error("paste.unsupported_control");
  const lines = text.split(/\r\n|[\r\n\u2028\u2029]/u).length;
  return Object.freeze({
    bytes: bytes.length,
    lines,
    confirmation:
      lines > 1 ||
      /[\t\u061c\u200b-\u200f\u2028-\u202e\u2060-\u206f\ufeff]/u.test(text),
  });
}
export type PasteOutcome = "submitted" | "cancelled" | "busy";
/** UI decision only, never native execution authority. No automatic resend. */
export class PasteGate {
  private pending: AbortController | undefined;
  private closed = false;
  private readonly abort = () => this.close();
  constructor(
    private readonly signal: AbortSignal,
    private readonly current: () => boolean,
    private readonly confirm: (
      text: string,
      assessment: PasteAssessment,
      signal: AbortSignal,
    ) => Promise<boolean>,
    private readonly send: (text: string) => void,
  ) {
    signal.addEventListener("abort", this.abort, { once: true });
    if (signal.aborted) this.close();
  }
  async submit(text: string): Promise<PasteOutcome> {
    if (this.closed || !this.current()) return "cancelled";
    if (this.pending) return "busy";
    const assessment = assessPaste(text),
      request = new AbortController();
    this.pending = request;
    try {
      if (assessment.confirmation) {
        let accepted = false;
        try {
          accepted = await this.confirm(text, assessment, request.signal);
        } catch {
          return "cancelled";
        }
        if (!accepted) return "cancelled";
      }
      if (this.closed || request.signal.aborted || !this.current())
        return "cancelled";
      this.send(text);
      return "submitted";
    } finally {
      if (this.pending === request) this.pending = undefined;
    }
  }
  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.signal.removeEventListener("abort", this.abort);
    this.pending?.abort();
  }
}

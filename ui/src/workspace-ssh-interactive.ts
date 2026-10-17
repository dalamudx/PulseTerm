import { uiText } from "./workspace-language";
export type InteractivePrompt = Readonly<{
  version: 1;
  round: string;
  name: string;
  instructions: string;
  prompts: readonly Readonly<{ text: string; echo: boolean }>[];
}>;
const uuid4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
function object(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
function bytes(value: string) {
  return new TextEncoder().encode(value).byteLength;
}
export function decodeInteractivePrompt(value: unknown): InteractivePrompt {
  if (
    !object(value) ||
    Object.keys(value).length !== 5 ||
    value.version !== 1 ||
    typeof value.round !== "string" ||
    !uuid4.test(value.round) ||
    typeof value.name !== "string" ||
    bytes(value.name) > 1024 ||
    typeof value.instructions !== "string" ||
    bytes(value.instructions) > 4096 ||
    !Array.isArray(value.prompts) ||
    value.prompts.length > 8
  )
    throw Error("ssh.session_protocol");
  const prompts = value.prompts.map((p) => {
    if (
      !object(p) ||
      Object.keys(p).length !== 2 ||
      typeof p.text !== "string" ||
      bytes(p.text) > 1024 ||
      typeof p.echo !== "boolean"
    )
      throw Error("ssh.session_protocol");
    return Object.freeze({ text: p.text, echo: p.echo });
  });
  if (prompts.reduce((sum, p) => sum + bytes(p.text), 0) > 8192)
    throw Error("ssh.session_protocol");
  return Object.freeze({
    version: 1,
    round: value.round,
    name: value.name,
    instructions: value.instructions,
    prompts: Object.freeze(prompts),
  });
}
/** Server text is literal and untrusted. Switching language never submits or recreates inputs. */
export function answerInteractivePrompt(
  dialog: HTMLDialogElement,
  prompt: InteractivePrompt,
  signal: AbortSignal,
): Promise<string[] | undefined> {
  if (signal.aborted || !dialog.open) return Promise.resolve(undefined);
  const doc = dialog.ownerDocument,
    form = doc.createElement("form"),
    heading = doc.createElement("h3"),
    warning = doc.createElement("p");
  form.className = "workspace-interactive-form";
  uiText(heading, "服务器认证问题");
  uiText(
    warning,
    "以下内容由服务器提供。仅在核对目标后回答；取消将关闭待认证连接。",
  );
  const remote = doc.createElement("pre");
  remote.className = "workspace-interactive-remote";
  remote.textContent = [prompt.name, prompt.instructions]
    .filter(Boolean)
    .join("\n");
  form.append(heading, warning, remote);
  const inputs = prompt.prompts.map((question) => {
    const label = doc.createElement("label"),
      input = doc.createElement("input");
    input.type = question.echo ? "text" : "password";
    input.autocomplete = "off";
    input.spellcheck = false;
    input.maxLength = 4096;
    input.setAttribute("autocapitalize", "off");
    label.append(doc.createTextNode(question.text), input);
    form.append(label);
    return input;
  });
  const status = doc.createElement("p"),
    submit = doc.createElement("button");
  status.setAttribute("role", "status");
  submit.type = "submit";
  uiText(submit, "提交本轮回答");
  form.append(status, submit);
  dialog.append(form);
  return new Promise((resolve) => {
    let done = false;
    const finish = (answers: string[] | undefined) => {
      if (done) return;
      done = true;
      signal.removeEventListener("abort", abort);
      for (const input of inputs) input.value = "";
      form.remove();
      resolve(answers);
    };
    const abort = () => finish(undefined);
    signal.addEventListener("abort", abort, { once: true });
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      event.stopPropagation();
      if (signal.aborted || !dialog.open) {
        finish(undefined);
        return;
      }
      const answers = inputs.map((input) => input.value);
      if (
        answers.some((a) => bytes(a) > 4096 || a.includes("\0")) ||
        answers.reduce((sum, a) => sum + bytes(a), 0) > 16384
      ) {
        answers.fill("");
        uiText(status, "回答超出长度限制，请缩短后再提交。");
        return;
      }
      finish(answers);
    });
    (inputs[0] ?? submit).focus();
  });
}

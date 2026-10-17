import { commandsEnglish } from "./i18n/commands.en";
import { connectionEnglish } from "./i18n/connection.en";
import { customProfilesEnglish } from "./i18n/custom-profiles.en";
import { identityEnglish } from "./i18n/identity.en";
import { profileOrganizationEnglish } from "./i18n/profile-organization.en";
import { settingsEnglish } from "./i18n/settings.en";
import { sshEnglish } from "./i18n/ssh.en";
import { terminalEnglish } from "./i18n/terminal.en";
import { workspaceEnglish } from "./i18n/workspace.en";
export type UiLanguage = "zh-CN" | "en";
export const englishMessages = {
  ...settingsEnglish,
  ...workspaceEnglish,
  ...sshEnglish,
  ...identityEnglish,
  ...profileOrganizationEnglish,
  ...connectionEnglish,
  ...terminalEnglish,
  ...commandsEnglish,
  ...customProfilesEnglish,
} as const;
export type UiMessageKey = keyof typeof englishMessages;
export type UiMessage = Readonly<{
  source: string;
  args: readonly (string | number | UiMessage)[];
}>;
export function message(
  source: string,
  ...args: (string | number | UiMessage)[]
): UiMessage {
  return { source, args };
}
export function isUiLanguage(value: unknown): value is UiLanguage {
  return value === "zh-CN" || value === "en";
}
export function englishText(source: string): string {
  return Object.hasOwn(englishMessages, source)
    ? englishMessages[source as UiMessageKey]
    : source;
}
type Binding = Readonly<{ ref: WeakRef<Node>; values: Map<string, UiMessage> }>;
type LanguageState = {
  language: UiLanguage;
  bindings: WeakMap<Node, Binding>;
  refs: Set<WeakRef<Node>>;
  signal?: AbortSignal;
  disposed: boolean;
};
const documents = new WeakMap<Document, LanguageState>();
function state(doc: Document): LanguageState {
  let value = documents.get(doc);
  if (!value) {
    value = {
      language: "zh-CN",
      bindings: new WeakMap(),
      refs: new Set(),
      disposed: false,
    };
    documents.set(doc, value);
  }
  return value;
}
export function renderUiMessage(
  language: UiLanguage,
  value: UiMessage,
): string {
  const template = language === "en" ? englishText(value.source) : value.source;
  return template.replace(/\{(\d+)\}/g, (_, index: string) => {
    const arg = value.args[Number(index)];
    return typeof arg === "object"
      ? renderUiMessage(language, arg)
      : String(arg ?? "");
  });
}
export function translate(
  doc: Document,
  key: UiMessageKey,
  ...args: (string | number | UiMessage)[]
): string {
  return renderUiMessage(state(doc).language, message(key, ...args));
}
function remember(node: Node, property: string, value: UiMessage): void {
  const s = state(node.ownerDocument as Document);
  if (s.disposed) return;
  let binding = s.bindings.get(node);
  if (!binding) {
    const ref = new WeakRef(node);
    binding = { ref, values: new Map() };
    s.bindings.set(node, binding);
    s.refs.add(ref);
  }
  binding.values.set(property, value);
}
function writeAttribute(
  element: HTMLElement,
  property: string,
  value: string,
): void {
  if (property === "value") (element as HTMLInputElement).value = value;
  else if (property === "aria-label") element.ariaLabel = value;
  else if (property === "title") element.title = value;
  else if (property === "placeholder")
    (element as HTMLInputElement).placeholder = value;
  else element.setAttribute(property, value);
}
/** Bind developer-owned messages only. Interpolation arguments remain literal user data. */
export function uiText(element: HTMLElement, value: string | UiMessage): void {
  const m = typeof value === "string" ? message(value) : value;
  element.textContent = renderUiMessage(
    state(element.ownerDocument).language,
    m,
  );
  const text = element.firstChild;
  if (text?.nodeType === 3) remember(text, "text", m);
}
export function uiNode(
  doc: Document,
  value: string | UiMessage,
): Text | string {
  const m = typeof value === "string" ? message(value) : value;
  const text = renderUiMessage(state(doc).language, m);
  if (typeof doc.createTextNode !== "function") return text;
  const node = doc.createTextNode(text);
  remember(node, "text", m);
  return node;
}
export function uiContent(element: HTMLElement): UiMessage {
  const node = element.firstChild;
  return (
    (node &&
      state(element.ownerDocument).bindings.get(node)?.values.get("text")) ??
    message(element.textContent ?? "")
  );
}
export function uiAttribute(
  element: HTMLElement,
  property: string,
  value: string | UiMessage,
): void {
  const m = typeof value === "string" ? message(value) : value;
  writeAttribute(
    element,
    property,
    renderUiMessage(state(element.ownerDocument).language, m),
  );
  remember(element, property, m);
}
/** Capture static shell text once, before saved names and terminal output are mounted. */
export function bindStaticLanguage(root: HTMLElement): void {
  const doc = root.ownerDocument;
  const walker = doc.createTreeWalker(root, 4);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const raw = node.nodeValue ?? "",
      source = raw.trim();
    if (!Object.hasOwn(englishMessages, source)) continue;
    const m = message(
      "{0}{1}{2}",
      raw.slice(0, raw.indexOf(source)),
      message(source),
      raw.slice(raw.indexOf(source) + source.length),
    );
    remember(node, "text", m);
    node.nodeValue = renderUiMessage(state(doc).language, m);
  }
  for (const element of [
    root,
    ...root.querySelectorAll<HTMLElement>(
      "[aria-label], [title], [placeholder]",
    ),
  ]) {
    for (const attr of ["aria-label", "title", "placeholder"]) {
      const source = element.getAttribute(attr);
      if (source && Object.hasOwn(englishMessages, source))
        uiAttribute(element, attr, source);
    }
  }
}
export function applyUiLanguage(
  doc: Document,
  language: UiLanguage,
  signal?: AbortSignal,
): boolean {
  const s = state(doc);
  if (!isUiLanguage(language) || s.disposed || signal?.aborted) return false;
  const changed = s.language !== language;
  s.language = language;
  doc.documentElement.lang = language;
  if (changed)
    for (const ref of s.refs) {
      const node = ref.deref();
      if (!node?.isConnected) {
        s.refs.delete(ref);
        continue;
      }
      const values = s.bindings.get(node)?.values;
      if (!values) continue;
      for (const [property, m] of values) {
        const text = renderUiMessage(language, m);
        if (property === "text") node.nodeValue = text;
        else writeAttribute(node as HTMLElement, property, text);
      }
    }
  if (signal && !s.signal) {
    s.signal = signal;
    signal.addEventListener(
      "abort",
      () => {
        s.disposed = true;
        s.refs.clear();
      },
      { once: true },
    );
  }
  return true;
}

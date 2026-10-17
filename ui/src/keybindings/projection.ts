import { workspaceKeybindingCommandIds } from "../workspace-command-catalog";
import {
  compileKeybindingDefinitions,
  type KeybindingAction,
  type KeybindingDefinition,
} from "./definitions";

const actions: readonly KeybindingAction[] = [
  ...workspaceKeybindingCommandIds,
  "palette",
];
export interface KeymapSource {
  readonly action: KeybindingAction;
  readonly source: "default" | "user";
  readonly disabled: boolean;
}
export interface KeymapProjection {
  readonly version: 1;
  readonly bindings: readonly KeybindingDefinition[];
  readonly sources: readonly KeymapSource[];
}
function record(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("keymap.invalid_projection");
  const prototype = Object.getPrototypeOf(value);
  if (
    (prototype !== Object.prototype && prototype !== null) ||
    Object.keys(value).length !== keys.length ||
    keys.some((key) => !Object.hasOwn(value, key)) ||
    Object.keys(value).some((key) => !keys.includes(key)) ||
    Object.values(Object.getOwnPropertyDescriptors(value)).some(
      (descriptor) => !("value" in descriptor),
    )
  )
    throw new Error("keymap.invalid_projection");
  return value as Record<string, unknown>;
}
function action(value: unknown): KeybindingAction {
  if (typeof value !== "string" || !actions.some((action) => action === value))
    throw new Error("keymap.invalid_projection");
  return value as KeybindingAction;
}
/** Read-only native drawing/selection metadata. No startup, credential, or Capability permission. */
export function decodeKeymapProjection(value: unknown): KeymapProjection {
  const root = record(value, ["version", "bindings", "sources"]);
  if (
    root.version !== 1 ||
    !Array.isArray(root.sources) ||
    root.sources.length !== actions.length ||
    !Array.isArray(root.bindings) ||
    root.bindings.length > 16
  )
    throw new Error("keymap.invalid_projection");
  const bindings = compileKeybindingDefinitions(
    root.bindings.map((value) => record(value, ["action", "keys", "when"])),
  );
  const seen = new Set<KeybindingAction>();
  const sources = root.sources.map((value) => {
    const source = record(value, ["action", "source", "disabled"]),
      id = action(source.action);
    if (
      seen.has(id) ||
      (source.source !== "default" && source.source !== "user") ||
      typeof source.disabled !== "boolean" ||
      source.disabled !== !bindings.some((binding) => binding.action === id)
    )
      throw new Error("keymap.invalid_projection");
    if (
      source.source === "default" &&
      source.disabled !== (id !== "palette" && id !== "find")
    )
      throw new Error("keymap.invalid_projection");
    seen.add(id);
    return Object.freeze({
      action: id,
      source: source.source,
      disabled: source.disabled,
    }) as KeymapSource;
  });
  return Object.freeze({
    version: 1,
    bindings,
    sources: Object.freeze(sources),
  });
}

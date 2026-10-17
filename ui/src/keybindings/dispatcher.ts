import { type ChordKeyEvent, matchesChord } from "./chords";
import { isCompositionKey } from "./composition";
import {
  type BindingContext,
  bindingConditionMatches,
  bindingContextFields,
} from "./conditions";
import {
  compileKeybindingDefinitions,
  type KeybindingAction,
  type KeybindingDefinition,
} from "./definitions";
export const KEY_SEQUENCE_DEADLINE_MS = 800;
export interface BindingKeyEvent extends ChordKeyEvent {
  readonly repeat: boolean;
  preventDefault(): void;
  stopImmediatePropagation(): void;
}
interface Capture {
  readonly nonce: string;
  readonly focus: string;
  readonly context: BindingContext;
}
interface Pending {
  readonly capture: Capture;
  readonly bindings: readonly KeybindingDefinition[];
  readonly deadline: number;
}
export interface KeybindingDispatcherOptions {
  readonly signal: AbortSignal;
  nonce(): string | undefined;
  focus(): string | undefined;
  context(): BindingContext;
  available(action: KeybindingAction): boolean;
  /** Invoke an existing UI action only; callback must independently recheck native authority. */
  execute(action: KeybindingAction): void;
  now?(): number;
}
/** Bounded UI gestures. No native permission, delayed action, timer callback, or Shell replay. */
export class KeybindingDispatcher {
  private bindings: readonly KeybindingDefinition[];
  private pending: Pending | undefined;
  private retired: Pending | undefined;
  private held = new Set<string>();
  private disposed = false;
  private lastNow = -Infinity;
  private readonly abort = () => this.dispose();
  constructor(
    source: unknown,
    private readonly options: KeybindingDispatcherOptions,
  ) {
    this.bindings = compileKeybindingDefinitions(source);
    options.signal.addEventListener("abort", this.abort, { once: true });
    if (options.signal.aborted) this.dispose();
  }
  /** Failed replacements retain the last valid table; successful replacement retires prefixes. */
  replace(source: unknown): void {
    const next = compileKeybindingDefinitions(source);
    if (this.disposed) return;
    this.cancelPrefix();
    this.bindings = next;
  }
  private capture(): Capture | undefined {
    if (this.disposed || this.options.signal.aborted) return;
    const nonce = this.options.nonce(),
      focus = this.options.focus(),
      context = this.options.context();
    if (
      !nonce ||
      !focus ||
      !bindingContextFields.every(
        (field) => typeof context[field] === "boolean",
      )
    )
      return;
    return { nonce, focus, context: Object.freeze({ ...context }) };
  }
  private current(capture: Capture): boolean {
    const current = this.capture();
    return (
      !!current &&
      current.nonce === capture.nonce &&
      current.focus === capture.focus &&
      bindingContextFields.every(
        (field) => current.context[field] === capture.context[field],
      )
    );
  }
  private consume(event: BindingKeyEvent): void {
    event.preventDefault();
    event.stopImmediatePropagation();
    this.held.add(event.key.toLowerCase());
  }
  keydown(event: BindingKeyEvent): boolean {
    if (this.disposed || this.options.signal.aborted) return false;
    if (
      isCompositionKey(event) ||
      event.defaultPrevented ||
      event.getModifierState?.("AltGraph")
    ) {
      this.pending = undefined;
      return false;
    }
    // Physical modifiers between chords are not sequence completion attempts.
    // Keep the original deadline and retired-completion guard unchanged.
    if (["Control", "Shift", "Alt", "Meta", "OS"].includes(event.key))
      return false;
    if (this.held.has(event.key.toLowerCase())) {
      this.consume(event);
      return true;
    }
    const now = (this.options.now ?? (() => performance.now()))();
    if (!Number.isFinite(now) || now < this.lastNow) {
      this.pending = undefined;
      return false;
    }
    this.lastNow = now;
    const retired = this.retired;
    this.retired = undefined;
    if (
      retired &&
      now < retired.deadline &&
      retired.bindings.some((binding) => {
        const chord = binding.sequence[1];
        return !!chord && matchesChord(chord, event);
      })
    )
      return false;
    const pending = this.pending;
    if (pending) {
      this.pending = undefined;
      // Never reinterpret an old completion as a new-context standalone command.
      if (now >= pending.deadline || !this.current(pending.capture))
        return false;
      const binding = pending.bindings.find((binding) => {
        const chord = binding.sequence[1];
        return (
          !!chord &&
          matchesChord(chord, event) &&
          this.options.available(binding.action) &&
          bindingConditionMatches(binding.when, pending.capture.context)
        );
      });
      if (!binding || event.repeat) return false;
      this.consume(event);
      if (
        this.current(pending.capture) &&
        this.options.available(binding.action)
      )
        this.options.execute(binding.action);
      return true;
    }
    if (event.repeat) return false;
    const capture = this.capture();
    if (!capture) return false;
    const candidates = this.bindings.filter((binding) => {
      const chord = binding.sequence[0];
      return (
        !!chord &&
        matchesChord(chord, event) &&
        bindingConditionMatches(binding.when, capture.context) &&
        this.options.available(binding.action)
      );
    });
    if (!candidates.length) return false;
    this.consume(event);
    if (!this.current(capture)) return true;
    const complete = candidates.find(
      (binding) => binding.sequence.length === 1,
    );
    if (complete) {
      if (this.options.available(complete.action))
        this.options.execute(complete.action);
    } else
      this.pending = {
        capture,
        bindings: candidates,
        deadline: now + KEY_SEQUENCE_DEADLINE_MS,
      };
    return true;
  }
  /** Consume release even if modifiers/focus changed after an owned keydown. */
  keyup(event: BindingKeyEvent): boolean {
    if (
      this.disposed ||
      this.options.signal.aborted ||
      !this.held.delete(event.key.toLowerCase())
    )
      return false;
    event.preventDefault();
    event.stopImmediatePropagation();
    return true;
  }
  /** Focus/source/fit transitions can proactively cancel prefixes, without replaying input. */
  cancelPrefix(): void {
    if (this.pending) this.retired = this.pending;
    this.pending = undefined;
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.pending = undefined;
    this.retired = undefined;
    this.held.clear();
    this.options.signal.removeEventListener("abort", this.abort);
  }
}

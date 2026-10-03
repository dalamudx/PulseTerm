interface Binding {
  listener: (shown: boolean) => void;
  signal: AbortSignal;
  stop: () => void;
}
/** Pane-local projection from pool visibility to the current attachment controller.
 * Subscription survives surface retirement until resume handoff. No backend token. */
export class PaneVisibility {
  private shown = true;
  private current: Binding | undefined;
  read(): boolean {
    return this.shown;
  }
  observe(shown: boolean): boolean {
    const old = this.shown;
    this.shown = shown;
    const current = this.current;
    if (!current || current.signal.aborted) return false;
    if (old !== shown) {
      try {
        current.listener(shown);
      } catch {
        current.stop();
        throw new Error("pane.visibility_failed");
      }
    }
    return true;
  }
  bind(listener: (shown: boolean) => void, signal: AbortSignal): () => void {
    if (signal.aborted) return () => {};
    this.current?.stop();
    const binding: Binding = {
      listener,
      signal,
      stop: () => {
        signal.removeEventListener("abort", binding.stop);
        if (this.current === binding) this.current = undefined;
      },
    };
    this.current = binding;
    signal.addEventListener("abort", binding.stop, { once: true });
    try {
      listener(this.shown);
    } catch {
      binding.stop();
      throw new Error("pane.visibility_failed");
    }
    return binding.stop;
  }
}

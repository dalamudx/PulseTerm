import type { checkConfigSource } from "./workspace-ssh-config";

/** Checks only an existing preview; never refreshes its ticket or chooses a target. */
export class ConfigSourceMonitor {
  private epoch = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private operation: AbortController | undefined;
  constructor(
    private readonly ports: {
      check: typeof checkConfigSource;
      visible(): boolean;
      checking(value: boolean): void;
      invalid(reason: "changed" | "unavailable"): void;
    },
  ) {}
  stop(): void {
    this.epoch++;
    clearTimeout(this.timer);
    this.timer = undefined;
    this.operation?.abort();
    this.operation = undefined;
    this.ports.checking(false);
  }
  start(nonce: string, token: string, current: () => boolean): void {
    this.stop();
    const epoch = this.epoch;
    const live = () => this.epoch === epoch && current();
    const schedule = () => {
      if (live())
        this.timer = setTimeout(() => {
          void tick();
        }, 2000);
    };
    const tick = async () => {
      this.timer = undefined;
      if (!live()) return;
      if (!this.ports.visible()) {
        schedule();
        return;
      }
      const operation = new AbortController();
      this.operation = operation;
      this.ports.checking(true);
      let invalid: "changed" | "unavailable" | undefined;
      try {
        const status = await this.ports.check(nonce, token, operation.signal);
        if (status === "changed") invalid = "changed";
      } catch (error) {
        // Other authorized reads can temporarily hold the shared native slot.
        if (error !== "workspace.busy") invalid = "unavailable";
      } finally {
        if (this.epoch === epoch) {
          this.operation = undefined;
          this.ports.checking(false);
        }
      }
      if (!live()) return;
      if (invalid) {
        this.stop();
        this.ports.invalid(invalid);
      } else schedule();
    };
    schedule();
  }
}

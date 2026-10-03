import type { NativeVisibilitySample } from "../../ipc/gen/commands";

function validate(s: NativeVisibilitySample): void {
  if (
    typeof s.incarnation !== "string" ||
    !s.incarnation ||
    s.incarnation.length > 64 ||
    !Number.isInteger(s.revision) ||
    s.revision < 1 ||
    s.revision > 0xffffffff ||
    typeof s.visible !== "boolean" ||
    typeof s.minimized !== "boolean" ||
    typeof s.valid !== "boolean"
  )
    throw new Error("native.invalid_sample");
}
/** Native identity is projection metadata, never a control capability. Bootstrap
 * with an owner-authorized snapshot after listening; stale events cannot undo it. */
export class NativeVisibilityFilter {
  private readonly incarnation: string;
  private revision: number;
  constructor(snapshot: NativeVisibilitySample) {
    validate(snapshot);
    if (!snapshot.valid) throw new Error("native.invalid_snapshot");
    this.incarnation = snapshot.incarnation;
    this.revision = snapshot.revision;
  }
  accept(sample: NativeVisibilitySample): NativeVisibilitySample | null {
    validate(sample);
    if (
      sample.incarnation !== this.incarnation ||
      sample.revision <= this.revision
    )
      return null;
    this.revision = sample.revision;
    return sample;
  }
}
/** Listen-before-snapshot race buffer. Keep latest per incarnation, never let a
 * reordered or foreign event overwrite current native state. Bounded, fail closed. */
export class NativeVisibilityBuffer {
  private pending = new Map<string, NativeVisibilitySample>();
  add(sample: NativeVisibilitySample): void {
    validate(sample);
    const old = this.pending.get(sample.incarnation);
    if (old && old.revision >= sample.revision) return;
    if (!old && this.pending.size >= 4)
      throw new Error("native.bootstrap_overload");
    this.pending.set(sample.incarnation, { ...sample });
  }
  take(incarnation: string): NativeVisibilitySample | undefined {
    const sample = this.pending.get(incarnation);
    this.pending.clear();
    return sample;
  }
}

export function nativeShown(sample: NativeVisibilitySample): boolean {
  return sample.valid && sample.visible && !sample.minimized;
}

import type { ViewScope } from "./view-host";
/** Finite validation/control deadline; hidden production views should not use
 * this as an eviction policy. Retirement clears the timer, old callbacks ignored. */
export function armViewDeadline(
  scope: ViewScope,
  ms: number,
  code: string,
  expired: (error: Error) => void,
): void {
  if (
    !Number.isInteger(ms) ||
    ms < 1 ||
    ms > 2147483647 ||
    !/^[a-z0-9_.]+$/.test(code)
  )
    throw new Error("view.deadline_invalid");
  const timer = setTimeout(() => {
    if (scope.isCurrent()) expired(new Error(code));
  }, ms);
  scope.own(() => clearTimeout(timer));
}

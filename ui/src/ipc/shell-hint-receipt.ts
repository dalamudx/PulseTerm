import type { ShellHints } from "./gen/commands";
// Sampling coordinates are not marker positions or proof of execution/rendering.
export function assertShellHintReceipt(
  value: ShellHints,
  initialPage: boolean,
): void {
  const decimal = (s: string) =>
    typeof s === "string" &&
    /^(0|[1-9]\d{0,19})$/.test(s) &&
    BigInt(s) <= 18446744073709551615n;
  if (
    !value ||
    typeof value.loginFixture !== "boolean" ||
    value.initialPage !== initialPage ||
    value.dropped !== "0" ||
    !decimal(value.sampledRevision) ||
    !decimal(value.sampledSourceBytes) ||
    !decimal(value.sampledDisplayBytes) ||
    !Array.isArray(value.markers) ||
    value.markers.length > 128
  )
    throw new Error("peer.shell_hint_receipt");
  const expected =
    value.loginFixture && initialPage
      ? ["promptStart", "promptEnd", "commandStart"]
      : [];
  if (
    value.markers.length !== expected.length ||
    value.markers.some(
      (marker, i) =>
        !marker ||
        marker.kind !== expected[i] ||
        Object.keys(marker).length !== 1,
    )
  )
    throw new Error("peer.shell_hint_sequence");
}

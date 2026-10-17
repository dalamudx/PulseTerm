import type { Geometry } from "./fit-scheduler";

export type LayoutBasis =
  | "local-native-confirmed"
  | "ssh-request-admitted"
  | "unchanged";
export interface LayoutReceipt extends Geometry {
  readonly basis: LayoutBasis;
}
/** Display metadata only. Never authorizes input or asserts remote kernel geometry. */
export function parseLayoutReceipt(
  value: unknown,
  requested: Readonly<Geometry>,
): LayoutReceipt {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("fit.invalid_receipt");
  const fields = Object.getOwnPropertyDescriptors(value);
  if (
    Object.keys(fields).length !== 3 ||
    !["columns", "rows", "basis"].every(
      (k) => fields[k] && "value" in fields[k],
    )
  )
    throw new Error("fit.invalid_receipt");
  const { columns, rows, basis } = value as Record<string, unknown>;
  if (
    !Number.isInteger(columns) ||
    !Number.isInteger(rows) ||
    columns !== requested.columns ||
    rows !== requested.rows ||
    requested.columns < 2 ||
    requested.columns > 160 ||
    requested.rows < 2 ||
    requested.rows > 60 ||
    typeof basis !== "string" ||
    !["local-native-confirmed", "ssh-request-admitted", "unchanged"].includes(
      basis,
    )
  )
    throw new Error("fit.invalid_receipt");
  return {
    columns: requested.columns,
    rows: requested.rows,
    basis: basis as LayoutBasis,
  };
}

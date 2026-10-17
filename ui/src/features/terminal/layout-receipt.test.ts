import { describe, expect, it } from "vitest";
import { parseLayoutReceipt } from "./layout-receipt";

const size = { columns: 80, rows: 24 };
describe("layout receipt", () => {
  it.each(["local-native-confirmed", "ssh-request-admitted", "unchanged"])(
    "accepts explicit %s without remote proof",
    (basis) => {
      expect(parseLayoutReceipt({ ...size, basis }, size)).toEqual({
        ...size,
        basis,
      });
    },
  );
  it.each([
    undefined,
    null,
    [],
    {},
    { ...size, basis: "remote-kernel-confirmed" },
    { ...size, basis: true },
    { ...size, basis: "ssh-request-admitted", columns: 81 },
    { ...size, basis: "ssh-request-admitted", rows: 24.5 },
    { ...size, basis: "ssh-request-admitted", authorized: true },
  ])("rejects missing, unknown or mismatching receipt %#", (value) =>
    expect(() => parseLayoutReceipt(value, size)).toThrow(
      "fit.invalid_receipt",
    ),
  );
  it("does not run a receipt getter", () => {
    let touched = false;
    const value = {
      columns: 80,
      rows: 24,
      get basis() {
        touched = true;
        return "ssh-request-admitted";
      },
    };
    expect(() => parseLayoutReceipt(value, size)).toThrow();
    expect(touched).toBe(false);
  });
});

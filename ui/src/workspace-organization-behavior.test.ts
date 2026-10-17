import { expect, it } from "vitest";
import { decodeBehaviorView } from "./workspace-organization-behavior";

const projection = () => ({
  version: 2,
  token: "11111111-1111-4111-8111-111111111111",
  data: {
    overrides: {
      terminal_preset: null,
      term_type: null,
      mac_option_as_meta: false,
      backspace_send: "bs",
      delete_send: null,
    },
    values: {
      terminal_preset: { value: "paper", source: "group" },
      term_type: { value: "vt100", source: "group" },
      mac_option_as_meta: { value: false, source: "host" },
      backspace_send: { value: "bs", source: "host" },
      delete_send: { value: "csi", source: "default" },
    },
  },
});
it("decodes sparse overrides without turning explicit false into inheritance", () => {
  const view = decodeBehaviorView(projection());
  expect(view.overrides.mac_option_as_meta).toBe(false);
  expect(view.overrides.term_type).toBeNull();
  expect(view.values.term_type).toEqual({ value: "vt100", source: "group" });
  expect(view.overrides.terminal_preset).toBeNull();
  expect(view.values.terminal_preset).toEqual({
    value: "paper",
    source: "group",
  });
});
it("rejects incomplete, unknown, invalid and mismatched scope projections", () => {
  for (const value of [
    null,
    [],
    { ...projection(), version: 1 },
    { ...projection(), token: "bad" },
    { ...projection(), unexpected: true },
    {
      ...projection(),
      data: {
        ...projection().data,
        overrides: {
          ...projection().data.overrides,
          terminal_preset: "#ffffff",
        },
      },
    },
    {
      ...projection(),
      data: {
        ...projection().data,
        overrides: Object.fromEntries(
          Object.entries(projection().data.overrides).filter(
            ([key]) => key !== "terminal_preset",
          ),
        ),
      },
    },
    {
      ...projection(),
      data: {
        ...projection().data,
        overrides: { ...projection().data.overrides, term_type: "bad" },
      },
    },
    {
      ...projection(),
      data: {
        ...projection().data,
        values: {
          ...projection().data.values,
          delete_send: { value: "del", source: "profile" },
        },
      },
    },
    {
      ...projection(),
      data: {
        ...projection().data,
        values: {
          ...projection().data.values,
          mac_option_as_meta: { value: "false", source: "host" },
        },
      },
    },
  ])
    expect(() => decodeBehaviorView(value)).toThrow();
});

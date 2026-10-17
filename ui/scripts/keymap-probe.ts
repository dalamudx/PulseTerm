import assert from "node:assert/strict";
import { readFileSync, statSync } from "node:fs";
import { compileKeybindingDefinitions } from "../src/keybindings/definitions";
import { decodeKeymapProjection } from "../src/keybindings/projection";

const path = process.argv[2];
assert(path, "dedicated Rust keymap corpus required");
assert(statSync(path).size <= 512 * 1024, "bounded interoperability input");
const corpus: unknown = JSON.parse(readFileSync(path, "utf8"));
assert(corpus && typeof corpus === "object");
const root = corpus as Record<string, unknown>;
assert.equal(root.version, 1);
assert(Array.isArray(root.projections) && root.projections.length === 8);
assert(Array.isArray(root.checks) && root.checks.length === 1158);
for (const projection of root.projections) decodeKeymapProjection(projection);
const defaults = (root.projections[0] as Record<string, unknown>).bindings;
assert(Array.isArray(defaults));
let accepted = 0;
for (const value of root.checks) {
  assert(value && typeof value === "object");
  const check = value as Record<string, unknown>;
  assert.equal(typeof check.accepted, "boolean");
  assert(check.binding && typeof check.binding === "object");
  const candidate = check.binding as Record<string, unknown>;
  let actual = true;
  try {
    compileKeybindingDefinitions([
      ...defaults.filter((binding) => binding.action !== candidate.action),
      candidate,
    ]);
  } catch {
    actual = false;
  }
  assert.equal(
    actual,
    check.accepted,
    "Rust/TypeScript keymap policy divergence",
  );
  if (actual) accepted++;
}
console.log(
  `keymap.actual_rust_projection_decoded projections=${root.projections.length} policy_checks=${root.checks.length} accepted=${accepted}`,
);

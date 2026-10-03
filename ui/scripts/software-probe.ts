import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { isDeepStrictEqual } from "node:util";
import { runXterm } from "./xterm-screen";

type Entry = {
  name: string;
  prefix: number[];
  suffix: number[];
  restore: number[] | null;
  gap: string | null;
  approximations: string[];
  strict: boolean;
  rustExact: boolean | null;
  expected: unknown;
  resized: { columns: number; rows: number } | null;
  risk: number;
};
const report = JSON.parse(
  readFileSync(
    new URL("../../target/probes/software.json", import.meta.url),
    "utf8",
  ),
) as { schemaVersion: number; columns: number; rows: number; cases: Entry[] };
assert.equal(report.schemaVersion, 1);
assert.equal(report.cases.length, 8);
const results = [];
for (const entry of report.cases) {
  const steps = (prefix: number[]) => [
    Uint8Array.from(prefix),
    ...(entry.resized ? [entry.resized] : []),
    Uint8Array.from(entry.suffix),
  ];
  const original = await runXterm(
    steps(entry.prefix),
    report.columns,
    report.rows,
  );
  const m2 = isDeepStrictEqual(original.screen, entry.expected);
  if (entry.restore === null || !entry.strict) {
    results.push({
      name: entry.name,
      status: entry.gap ?? "approximate",
      m2,
      risk: entry.risk,
      approximations: entry.approximations,
    });
    continue;
  }
  assert.equal(entry.rustExact, true, entry.name);
  const restored = await runXterm(
    steps(entry.restore),
    report.columns,
    report.rows,
  );
  const m1 = isDeepStrictEqual(restored.screen, entry.expected);
  const m3 = isDeepStrictEqual(restored.screen, original.screen);
  assert.equal(
    restored.replies.length,
    0,
    `${entry.name}: renderer emitted a backend-owned query response`,
  );
  results.push({
    name: entry.name,
    status: m1
      ? "authority-match"
      : m2
        ? "restore-fail"
        : "baseline-divergence",
    m1,
    m2,
    m3,
    risk: entry.risk,
  });
}
writeFileSync(
  new URL("../../target/probes/software-xterm.json", import.meta.url),
  `${JSON.stringify({ schemaVersion: 1, results }, null, 2)}\n`,
);
for (const result of results)
  console.log(
    result.name,
    result.status,
    `M2=${result.m2} risk=${result.risk}`,
  );
assert.equal(
  results.filter((r) => r.status === "restore-fail").length,
  0,
  "restore regressed where original xterm matched backend",
);
console.log(
  "PASS real software: supported Rust continuations and conditional xterm restores; unsupported/baseline cases explicitly reported",
);

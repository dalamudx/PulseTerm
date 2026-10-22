import {test} from "node:test";
import assert from "node:assert/strict";
import {settingsProbeAllowed, settingsReceiptAccepted, requiredSettingsTests} from "./probe-settings-file.mjs";

const rows = requiredSettingsTests.map(name => `test ${name} ... ok`);
const summary = "test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 80 filtered out;";
const stdout = [...rows, summary].join("\n");
const success = {status: 0, stdout};

test("settings probe admits only implemented platforms", () => {
  assert.equal(settingsProbeAllowed("linux"), true);
  assert.equal(settingsProbeAllowed("darwin"), true);
  for (const platform of ["win32", "freebsd", undefined]) assert.equal(settingsProbeAllowed(platform), false);
});

test("receipt requires every exact case and one complete summary", () => {
  assert.equal(settingsReceiptAccepted(success), true);
  assert.equal(settingsReceiptAccepted({...success, stdout: stdout.replaceAll("\n", "\r\n")}), true);
  for (const text of ["", stdout.replace(rows[0], ""), stdout.replace("17 passed", "0 passed"), stdout.replace("0 failed", "1 failed"), stdout.replace("0 ignored", "1 ignored"), stdout.replace(rows[1], `${rows[1]} extra`), `${stdout}\n${rows[0]}`, `${stdout}\n${summary}`]) {
    assert.equal(settingsReceiptAccepted({...success, stdout: text}), false);
  }
});

test("legacy scope cannot substitute for execution authorization cases", () => {
  const legacy = rows.slice(0, 12).concat("test result: ok. 12 passed; 0 failed; 1 ignored; 0 measured; 80 filtered out;").join("\n");
  assert.equal(settingsReceiptAccepted({...success, stdout: legacy}), false);
  const renamed = stdout.replace(requiredSettingsTests[12], "settings_file::authorization_tests::unrelated");
  assert.equal(settingsReceiptAccepted({...success, stdout: renamed}), false);
});

test("failure signal timeout and non-text are never success", () => {
  for (const result of [{...success, status: 1}, {...success, signal: "SIGTERM"}, {...success, error: new Error("timeout")}, {...success, stdout: null}]) {
    assert.equal(settingsReceiptAccepted(result), false);
  }
});

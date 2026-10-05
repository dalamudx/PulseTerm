import {test} from "node:test";
import assert from "node:assert/strict";
import {settingsProbeAllowed,settingsReceiptAccepted,requiredSettingsTests} from "./probe-settings-file.mjs";
const stdout=[...requiredSettingsTests,"test result: ok. 12 passed; 0 failed; 1 ignored; 0 measured; 80 filtered out;"].join("\n");
const success={status:0,stdout};
test("settings probe admits only implemented platforms",()=>{assert.equal(settingsProbeAllowed("linux"),true);assert.equal(settingsProbeAllowed("darwin"),true);for(const platform of ["win32","freebsd",undefined])assert.equal(settingsProbeAllowed(platform),false);});
test("receipt requires every exact case and real summary",()=>{assert.equal(settingsReceiptAccepted(success),true);assert.equal(settingsReceiptAccepted({...success,stdout:stdout.replaceAll("\n","\r\n")}),true);for(const text of ["",stdout.replace(requiredSettingsTests[0],""),stdout.replace("12 passed","0 passed"),stdout.replace("0 failed","1 failed"),stdout.replace("1 ignored","0 ignored"),stdout.replace(requiredSettingsTests[1],`${requiredSettingsTests[1]} extra`)])assert.equal(settingsReceiptAccepted({...success,stdout:text}),false);});
test("failure signal timeout and non-text are never success",()=>{for(const result of [{...success,status:1},{...success,signal:"SIGTERM"},{...success,error:new Error("timeout")},{...success,stdout:null}])assert.equal(settingsReceiptAccepted(result),false);});

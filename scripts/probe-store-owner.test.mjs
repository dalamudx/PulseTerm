import assert from "node:assert/strict";
import { test } from "node:test";
import { ownerProbeAllowed, ownerReceiptAccepted } from "./probe-store-owner.mjs";
const name="privacy_tests::root_foreign_owner_is_rejected_without_repairing_parent_database_or_sidecars";
const stdout=`test ${name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 22 filtered out; finished in 0.01s\n`;
test("only existing Linux/macOS effective UID zero is admitted",()=>{
  assert(ownerProbeAllowed("linux",0));assert(ownerProbeAllowed("darwin",0));
  for(const [platform,uid] of [["win32",0],["linux",1000],["linux","0"],["darwin",undefined],["freebsd",0]])assert.equal(ownerProbeAllowed(platform,uid),false);
});
test("exact successful fixture and result are both required",()=>{
  assert(ownerReceiptAccepted({status:0,stdout}));assert(ownerReceiptAccepted({status:0,stdout:stdout.replaceAll("\n","\r\n")}));
  for(const output of ["",stdout.replace(name,"unrelated_test"),stdout.replace("1 passed","0 passed"),stdout.replace("0 ignored","1 ignored"),stdout.replace(" ... ok"," ... ignored")])assert.equal(ownerReceiptAccepted({status:0,stdout:output}),false);
});
test("signals errors or unsuccessful exit cannot substitute a successful line",()=>{
  for(const result of [{status:1},{status:null},{status:"0"},{status:0,signal:"SIGTERM"},{status:0,error:new Error("timeout")}])assert.equal(ownerReceiptAccepted({...result,stdout}),false);
});

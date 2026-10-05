import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";
const TEST="privacy_tests::root_foreign_owner_is_rejected_without_repairing_parent_database_or_sidecars";
export function ownerProbeAllowed(platform, uid) {
  return (platform === "linux" || platform === "darwin") && uid === 0;
}
export function ownerReceiptAccepted(result) {
  if(result.status!==0 || result.signal || result.error || typeof result.stdout!=="string")return false;
  const lines=result.stdout.replaceAll("\r\n","\n").split("\n");
  return lines.includes(`test ${TEST} ... ok`) && lines.some(line=>line.startsWith("test result: ok. 1 passed; 0 failed; 0 ignored;"));
}
if(process.argv[1] && import.meta.url===pathToFileURL(process.argv[1]).href) {
  const uid=typeof process.geteuid ==="function"?process.geteuid():undefined;
  if(!ownerProbeAllowed(process.platform,uid)) {
    console.error("store-owner.unsupported: requires existing Linux/macOS effective UID 0; no privilege escalation");
    process.exitCode=2;
  } else {
    const result=spawnSync("cargo",["test","-p","pulse-store","--locked",TEST,"--","--exact","--ignored","--color=never"],{encoding:"utf8",timeout:120000,maxBuffer:256*1024,env:{...process.env,CARGO_TERM_COLOR:"never"}});
    if(ownerReceiptAccepted(result))console.log("PASS store-owner: actual foreign-UID disposable fixtures refused without repairing existing files");
    else {console.error("store-owner.invalid_receipt: exact successful root-fixture receipt required");process.exitCode=1;}
  }
}

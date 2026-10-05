import {spawnSync} from "node:child_process";
import {pathToFileURL} from "node:url";
export const requiredSettingsTests = [
"process_tests::two_independent_process_snapshots_cannot_overwrite_committed_settings",
"process_tests::process_kill_before_rename_preserves_original_and_releases_lock",
"process_tests::process_kill_after_rename_recovers_complete_new_text_without_claiming_power_durability",
"tests::shared_open_description_does_not_keep_completed_operation_locked",
"tests::create_save_reopen_remove_and_conflict_preserve_original_text",
"tests::cooperating_handles_are_busy_and_old_inode_does_not_revive",
"tests::invalid_size_utf8_and_schema_never_get_reset",
"tests::private_parent_file_and_lock_aliases_or_readonly_intent_are_rejected",
"tests::changed_during_preparation_and_foreign_directory_snapshot_are_refused",
"tests::first_install_never_overwrites_a_noncooperating_creation",
"tests::pre_install_failure_preserves_original_but_post_install_is_uncertain",
"tests::replaced_directory_and_replaced_lock_are_not_written",
].map(name=>`test settings_file::unix::${name} ... ok`);
export function settingsProbeAllowed(platform){return platform==="linux"||platform==="darwin";}
export function settingsReceiptAccepted(result) {
 if(result.status!==0||result.signal||result.error||typeof result.stdout!=="string")return false;
 const lines=result.stdout.replaceAll("\r\n","\n").split("\n");
 return requiredSettingsTests.every(line=>lines.includes(line))&&lines.some(line=>line.startsWith("test result: ok. 12 passed; 0 failed; 1 ignored;"));
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href) {
 if(!settingsProbeAllowed(process.platform)){console.error("settings-file.unsupported_platform");process.exitCode=2;}
 else {
 const result=spawnSync("cargo",["test","-p","pulse-store","--locked","--lib","settings_file","--","--color=never"],{encoding:"utf8",timeout:120000,maxBuffer:512*1024,env:{...process.env,CARGO_TERM_COLOR:"never"}});
 if(settingsReceiptAccepted(result))console.log("PASS settings-file: exact twelve cases, independent process conflict, process-kill atomicity, explicit shared-descriptor unlock");
 else {console.error("settings-file.invalid_receipt");process.exitCode=1;}
 }
}

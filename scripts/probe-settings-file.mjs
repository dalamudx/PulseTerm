import {spawnSync} from "node:child_process";
import {pathToFileURL} from "node:url";
import {verifyGroup} from "./verify-native-tests.mjs";

export const requiredSettingsTests = [
  ...[
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
  ].map(name => `settings_file::unix::${name}`),
  ...[
    "no_op_still_requires_execution_authorization",
    "revoked_at_entry_never_installs",
    "accepted_install_is_not_rolled_back_after_final_execution_grant",
    "revoked_after_temp_fsync_or_before_install_preserves_original_and_removes_temp",
    "panicking_execution_predicate_is_denied_and_cleans_temp",
  ].map(name => `settings_file::authorization_tests::${name}`),
];

export function settingsProbeAllowed(platform) {
  return platform === "linux" || platform === "darwin";
}

export function settingsReceiptAccepted(result) {
  if (result.status !== 0 || result.signal || result.error || typeof result.stdout !== "string") return false;
  try {
    verifyGroup(result.stdout.replaceAll("\r\n", "\n"), requiredSettingsTests);
    return true;
  } catch {
    return false;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (!settingsProbeAllowed(process.platform)) {
    console.error("settings-file.unsupported_platform");
    process.exitCode = 2;
  } else {
    // The ignored child fixture is executed explicitly by the process-kill parent tests.
    const result = spawnSync("cargo", ["test", "-p", "pulse-store", "--locked", "--lib", "settings_file", "--", "--color=never", "--skip", "settings_file::unix::process_tests::fixture_child"], {
      encoding: "utf8", timeout: 120000, maxBuffer: 512 * 1024,
      env: {...process.env, CARGO_TERM_COLOR: "never"},
    });
    if (settingsReceiptAccepted(result)) {
      console.log("PASS settings-file: exact seventeen cases, execution authorization, independent process conflict, process-kill atomicity and shared-descriptor unlock");
    } else {
      process.stdout.write(result.stdout ?? "");
      process.stderr.write(result.stderr ?? "");
      if (result.error) console.error(result.error.message);
      console.error("settings-file.invalid_receipt");
      process.exitCode = 1;
    }
  }
}

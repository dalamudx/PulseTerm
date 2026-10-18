#!/usr/bin/env python3
"""Receipt regressions: incomplete, stale, or content-bearing traces cannot pass."""
import json
import os
from pathlib import Path
import runpy
import tempfile
import unittest

verify = runpy.run_path("scripts/check-workspace-startup-traces.py")["verify"]
ORDER = "run_entered services_ready runtime_build_started runtime_built setup_entered instance_verified data_prepared window_build_started window_built visibility_installed window_published endpoint_published setup_finished ready page_started local_page_finished nonce_injected document_confirm_entered document_confirmed visibility_read profiles_requested profiles_listed cli_page_ready window_destroyed exit_requested exited".split()

class StartupTraceReceiptTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="trace-check-")
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.source, self.evidence, self.output = (root / name for name in ("source", "evidence", "output"))
        self.source.mkdir(mode=0o700)
        self.evidence.mkdir()
        for state, pid in (("locked", 100001), ("unlocked", 100002)):
            self.write_trace(pid, [{"version": 1, "pid": pid, "elapsed_us": index, "stage": stage} for index, stage in enumerate(ORDER)])
            receipt = {"passed": True, "state": state, "close_phase": "pending-dialog", "gdb_attached": False,
                       "planned_cycles": 1, "completed_cycles": 1,
                       "cycles": [{"pid": pid, "gui_exit": 0, "zero_auth_and_signatures": True,
                                   "source_hashes_unchanged": True, "endpoints_drained": True}]}
            (self.evidence / f"native-close-{state}-normal-pending-dialog-result.json").write_text(json.dumps(receipt))
    def write_trace(self, pid, events):
        path = self.source / f"startup-{pid}.jsonl"
        path.write_text("".join(json.dumps(event) + "\n" for event in events))
        path.chmod(0o600)
    def check(self):
        return verify(self.source, self.evidence, "pending-dialog", 1, self.output)
    def events(self):
        return [json.loads(line) for line in (self.source / "startup-100001.jsonl").read_text().splitlines()]
    def test_complete_exact_pid_pair_is_accepted_without_marking_issue_resolved(self):
        self.assertTrue(self.check())
        result = json.loads((self.output / "result.json").read_text())
        self.assertFalse(result["historical_startup_or_sigsegv_resolved"])
        self.assertEqual(result["actual_trace_count"], 2)
    def test_content_field_is_rejected_without_archiving_its_value(self):
        events = self.events()
        events[0]["payload"] = "DO_NOT_ARCHIVE_CONTENT"
        self.write_trace(100001, events)
        self.assertFalse(self.check())
        self.assertNotIn("DO_NOT_ARCHIVE_CONTENT", (self.output / "result.json").read_text())
        self.assertFalse((self.output / "traces" / "startup-100001.jsonl").exists())
    def test_stale_native_pid_set_is_rejected(self):
        events = self.events()
        for event in events:
            event["pid"] = 100003
        (self.source / "startup-100001.jsonl").unlink()
        self.write_trace(100003, events)
        self.assertFalse(self.check())
    def test_missing_endpoint_and_trace_over_budget_are_rejected(self):
        original = self.events()
        for stage in ("endpoint_published", "document_confirmed", "cli_page_ready"):
            self.write_trace(100001, [event for event in original if event["stage"] != stage])
            self.assertFalse(self.check())
        events = [{"version": 1, "pid": 100001, "elapsed_us": index, "stage": "page_started"} for index in range(65)]
        self.write_trace(100001, events)
        self.assertFalse(self.check())
    def test_failed_receipt_cannot_reuse_complete_traces(self):
        path = self.evidence / "native-close-locked-normal-pending-dialog-result.json"
        receipt = json.loads(path.read_text())
        receipt["passed"] = False
        path.write_text(json.dumps(receipt))
        self.assertFalse(self.check())

if __name__ == "__main__":
    assert Path(os.environ.get("TMPDIR", "")).is_relative_to(Path("/data/temp"))
    unittest.main()

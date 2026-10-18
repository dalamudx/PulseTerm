#!/usr/bin/env python3
"""Associate bounded fixed-stage startup traces with exact native close receipts."""
import argparse
import json
import os
from pathlib import Path
import shutil
import stat

STAGES = frozenset("run_entered configuration_denied services_ready services_failed runtime_build_started setup_entered instance_verified data_prepared window_build_started window_built page_started local_page_finished blank_page_finished other_page_finished page_rejected nonce_injected document_confirm_entered document_confirmed visibility_read profiles_requested profiles_listed cli_page_ready visibility_installed window_published endpoint_published setup_finished runtime_built runtime_build_failed ready exit_requested window_destroyed exited".split())
REQUIRED = frozenset("run_entered services_ready runtime_build_started setup_entered instance_verified data_prepared window_build_started window_built local_page_finished nonce_injected document_confirm_entered document_confirmed visibility_read profiles_requested profiles_listed cli_page_ready visibility_installed window_published endpoint_published setup_finished runtime_built ready exit_requested window_destroyed exited".split())
FAILURE = frozenset("configuration_denied services_failed runtime_build_failed page_rejected blank_page_finished other_page_finished".split())

def read_trace(path):
    metadata = path.lstat()
    assert stat.S_ISREG(metadata.st_mode), "trace_not_regular"
    assert metadata.st_nlink == 1 and metadata.st_uid == os.geteuid(), "trace_owner_or_links"
    assert stat.S_IMODE(metadata.st_mode) == 0o600, "trace_mode"
    assert 0 < metadata.st_size <= 64 * 256, "trace_bytes"
    pid = int(path.stem.removeprefix("startup-"))
    assert pid > 0 and path.name == f"startup-{pid}.jsonl", "trace_name"
    lines = path.read_text(encoding="utf-8").splitlines()
    assert 1 <= len(lines) <= 64 and all(len(line) <= 256 for line in lines), "trace_count_or_line"
    events = []
    previous = 0
    for line in lines:
        value = json.loads(line)
        assert isinstance(value, dict) and set(value) == {"version", "pid", "elapsed_us", "stage"}, "trace_schema"
        assert type(value["version"]) is int and value["version"] == 1, "trace_version"
        assert type(value["pid"]) is int and value["pid"] == pid, "trace_pid"
        assert type(value["elapsed_us"]) is int and value["elapsed_us"] >= previous, "trace_clock"
        assert isinstance(value["stage"], str) and value["stage"] in STAGES, "trace_stage"
        previous = value["elapsed_us"]
        events.append(value)
    return pid, events

def verify(source, evidence, phase, cycles, output):
    report = {"version": 1, "kind": "owned-primary-window-startup-stages", "passed": False,
              "historical_startup_or_sigsegv_resolved": False, "close_phase": phase,
              "planned_gui_count": cycles * 2, "traces": [], "errors": []}
    output.mkdir(parents=True, exist_ok=True)
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    expected = set()
    observed = set()
    try:
        assert source.is_absolute() and source.is_dir() and not source.is_symlink(), "trace_directory"
        for state in ("locked", "unlocked"):
            suffix = "" if phase == "settled" else f"-{phase}"
            path = evidence / f"native-close-{state}-normal{suffix}-result.json"
            receipt = json.loads(path.read_text())
            assert receipt["passed"] is True and receipt["state"] == state and receipt["close_phase"] == phase, "native_receipt"
            assert receipt["gdb_attached"] is False, "unexpected_gdb_observer"
            assert receipt["planned_cycles"] == receipt["completed_cycles"] == cycles == len(receipt["cycles"]), "native_cycles"
            for cycle in receipt["cycles"]:
                pid = cycle["pid"]
                assert type(pid) is int and pid > 0 and pid not in expected, "native_pid"
                assert cycle["gui_exit"] == 0, "native_exit"
                assert all(cycle[key] is True for key in ("zero_auth_and_signatures", "source_hashes_unchanged", "endpoints_drained")), "native_guards"
                expected.add(pid)
            shutil.copy2(path, output / path.name)
        traces_dir = output / "traces"
        traces_dir.mkdir(exist_ok=True)
        for path in sorted(source.iterdir()):
            pid, events = read_trace(path)
            assert pid not in observed, "duplicate_trace"
            observed.add(pid)
            stages = [event["stage"] for event in events]
            missing = sorted(REQUIRED - set(stages))
            failures = sorted(FAILURE & set(stages))
            report["traces"].append({"pid": pid, "events": len(events), "last_stage": stages[-1],
                                     "missing": missing, "failure_stages": failures,
                                     "last_elapsed_us": events[-1]["elapsed_us"]})
            shutil.copy2(path, traces_dir / path.name)
            assert stages[0] == "run_entered" and stages[-1] == "exited", "trace_lifetime"
            assert not missing and not failures, "trace_checkpoint"
            setup_order = "setup_entered instance_verified data_prepared window_build_started window_built visibility_installed window_published endpoint_published setup_finished".split()
            assert [stages.index(stage) for stage in setup_order] == sorted(stages.index(stage) for stage in setup_order), "trace_setup_order"
        assert observed == expected and len(observed) == cycles * 2, "trace_native_pid_set"
        report["passed"] = True
    except (AssertionError, OSError, ValueError, KeyError, TypeError) as error:
        report["errors"].append(str(error))
    report["actual_gui_count"] = len(expected)
    report["actual_trace_count"] = len(observed)
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    return report["passed"]

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--evidence", type=Path, default=Path("target/probes"))
    parser.add_argument("--phase", choices=("settled", "cancel-unobserved", "pending-dialog"), default="pending-dialog")
    parser.add_argument("--cycles", type=int, choices=range(1, 33), default=12)
    args = parser.parse_args()
    raise SystemExit(0 if verify(args.source, args.evidence, args.phase, args.cycles, args.output) else 1)

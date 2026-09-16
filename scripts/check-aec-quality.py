"""Run every native AEC test and verify the required quality-gate inventory.

The command includes ignored tests and the native alignment contracts. A successful
run establishes only these software gates, never speakerphone readiness.
No devices are opened. Logs and a JSON result are retained even on test failure.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
REQUIRED = {
    "aec_webrtc": {"tests::silence_formats_and_invalid_samples"},
    "offline": {"broadband_double_talk_acceptance"},
    "speech_quality": {
        "speech_like_double_talk_acceptance",
        "intermittent_echo_path_change_acceptance",
        "echo_gain_step_recovers_with_fixed_delay",
        "unrelated_render_preserves_local_speech",
        "varied_speech_like_double_talk_preserves_quiet_local_speech",
    },
    "robustness": {"stereo_delay_steps_with_reflections_and_speaker_distortion"},
}


def inventory(text):
    return {line.removesuffix(": test") for line in text.splitlines() if line.endswith(": test")}


def verdict(returncode, output, expected):
    """Require the whole discovered inventory to execute, with no ignored tests."""
    match = re.search(
        r"^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; "
        r"(\d+) measured; (\d+) filtered out;", output, re.MULTILINE
    )
    return bool(returncode == 0 and expected and match
                and tuple(map(int, match.groups())) == (len(expected), 0, 0, 0, 0))


def run_target(executable, required, log_dir, name):
    listing = subprocess.run([executable, "--list", "--format", "terse"], cwd=ROOT,
                             capture_output=True, text=True, check=False, timeout=60)
    (log_dir / f"{name}-inventory.log").write_text(listing.stdout + listing.stderr)
    discovered = inventory(listing.stdout)
    missing = required - discovered
    if listing.returncode or missing or not discovered:
        return {"status": "failed", "reason": "test inventory incomplete",
                "missing": sorted(missing), "returncode": listing.returncode}
    result = subprocess.run(
        [executable, "--include-ignored", "--nocapture", "--format", "pretty", "--color", "never"],
        cwd=ROOT, capture_output=True, text=True, check=False, timeout=180,
    )
    output = result.stdout + result.stderr
    (log_dir / f"{name}.log").write_text(output)
    return {"status": "passed" if verdict(result.returncode, output, discovered) else "failed",
            "returncode": result.returncode, "tests": sorted(discovered), "log": f"{name}.log"}


def check(log_dir):
    command = ["cargo", "test", "--locked", "-p", "aec-webrtc", "--tests", "--no-run",
               "--message-format=json"]
    build = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=False)
    (log_dir / "build.jsonl").write_text(build.stdout)
    (log_dir / "build.log").write_text(build.stderr)
    report = {"scope": "offline native AEC software gates only",
              "speakerphone_acceptance": "not_established",
              "host": {"system": platform.system(), "machine": platform.machine()},
              "build_command": command, "build_returncode": build.returncode,
              "status": "failed", "targets": {}}
    sources = ["crates/aec-webrtc/src/bridge.cc", "crates/aec-webrtc/src/config.h",
               "crates/aec-webrtc/src/render_alignment.h", "crates/aec-webrtc/src/lib.rs",
               "crates/aec-webrtc/tests/robustness.rs", "scripts/build-aec.py",
               "crates/aec-webrtc/tests/suppression_probe.cc", "scripts/probe-aec-suppression.py",
               "crates/aec-webrtc/tests/offline.rs", "crates/aec-webrtc/tests/speech_quality.rs",
               "scripts/check-aec-quality.py", "Cargo.lock", "third_party/aec/SHA256.json"]
    report["source_sha256"] = {
        path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in sources
    }
    if build.returncode:
        return report
    try:
        probe = subprocess.run(
            [sys.executable, str(ROOT / "scripts/probe-aec-suppression.py"), "debug", "--alignment"],
            cwd=ROOT, capture_output=True, text=True, check=False, timeout=120,
        )
        (log_dir / "native-alignment.log").write_text(probe.stdout + probe.stderr)
        report["native_alignment"] = {
            "status": "passed" if probe.returncode == 0 and
                "native_alignment_contracts=passed cases=7" in probe.stdout else "failed",
            "returncode": probe.returncode, "log": "native-alignment.log",
        }
    except (OSError, subprocess.TimeoutExpired) as error:
        report["native_alignment"] = {"status": "failed", "reason": str(error)}
    executables = {}
    for line in build.stdout.splitlines():
        event = json.loads(line)
        if event.get("reason") == "compiler-artifact" and event.get("executable"):
            if event.get("profile", {}).get("test"):
                executables[event["target"]["name"]] = event["executable"]
    # Execute all discovered test targets, not just those with known failures.
    for name in sorted(executables.keys() | REQUIRED.keys()):
        if name not in executables:
            result = {"status": "failed", "reason": "required test executable missing"}
        else:
            try:
                result = run_target(executables[name], REQUIRED.get(name, set()), log_dir, name)
            except (OSError, subprocess.TimeoutExpired) as error:
                result = {"status": "failed", "reason": str(error)}
        report["targets"][name] = result
        print(f"{name}: {result['status']}", flush=True)
    if report["native_alignment"]["status"] == "passed" and all(
        result["status"] == "passed" for result in report["targets"].values()
    ):
        report["status"] = "passed"
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path,
                        default=ROOT / "target/aec-acceptance" /
                        datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ"))
    args = parser.parse_args()
    # Preserve previous evidence; each run must use a new directory.
    args.output_dir.mkdir(parents=True, exist_ok=False)
    try:
        report = check(args.output_dir)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        report = {"status": "failed", "speakerphone_acceptance": "not_established",
                  "reason": str(error)}
    (args.output_dir / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Offline AEC quality: {report['status']}. Report: {args.output_dir / 'result.json'}")
    print("Room, processed-voice listening, timing and conferencing acceptance remain separate.")
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())

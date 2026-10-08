#!/usr/bin/env python3
"""Run explicit test inventories and preserve machine-readable evidence under target/."""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def validate_performance(report, profile):
    for field in ("frames", "p99Us", "missed", "modelErrors", "aecFrames", "burners", "skippedInferences"):
        value = report.get(field)
        if type(value) is not int or value < 0:
            raise ValueError(f"invalid performance field: {field}")
    if report["frames"] < profile["minFrames"] or report["aecFrames"] < profile["minAecFrames"]:
        raise ValueError("insufficient measured or native AEC frames")
    if report["missed"] > report["frames"] or report["modelErrors"] != 0:
        raise ValueError("invalid deadlines or model errors")
    if report.get("degraded") is not False or report["skippedInferences"] != 0:
        raise ValueError("benchmark skipped DFN inference or degraded to bypass")
    if report.get("paced") is not True or report["burners"] != profile["burners"]:
        raise ValueError("benchmark conditions do not match profile")
    if report["p99Us"] > profile["maxP99Us"]:
        raise ValueError("p99 exceeds reference hardware budget")
    if report["missed"] / report["frames"] > profile["maxMissRate"]:
        raise ValueError("deadline miss rate exceeds reference hardware budget")


def hardware():
    system = platform.system()
    if system == "Darwin":
        cpu = subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"], text=True).strip()
    elif system == "Linux":
        cpu = next(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name"))
    else:
        cpu = platform.processor()
    return {"system": system, "machine": platform.machine(), "cpu": cpu}


def run_test(output_dir, name, cargo_args, names, *, ignored=False, env=None, timeout=900):
    base = ["cargo", "test", "--release", "--locked", *cargo_args]
    listing = subprocess.check_output([*base, "--", "--list"], cwd=ROOT, text=True)
    inventory = {line.removesuffix(": test") for line in listing.splitlines() if line.endswith(": test")}
    if not set(names) <= inventory:
        raise ValueError(f"missing test inventory: {set(names) - inventory}")
    # Exact names prevent a renamed filter from silently succeeding with zero tests.
    results = []
    for test in names:
        command = [*base, test, "--", "--exact", "--nocapture", *(["--ignored"] if ignored else [])]
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=timeout)
        log = output_dir / f"{name}-{test.replace('::', '-')}.log"
        log.write_text(result.stdout)
        print(result.stdout, end="", flush=True)
        if result.returncode or not re.search(r"test result: ok\. 1 passed; 0 failed", result.stdout):
            raise RuntimeError(f"test failed or did not execute: {test}; see {log}")
        results.append(result.stdout)
    return "\n".join(results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tier", choices=["extended", "soak", "performance"])
    parser.add_argument("--seconds", type=int, default=7200)
    parser.add_argument("--profile")
    parser.add_argument("--output", type=Path, default=ROOT / "target/test-evidence")
    args = parser.parse_args()
    if not 1 <= args.seconds <= 7200:
        parser.error("seconds must be within 1..7200")
    args.output.mkdir(parents=True, exist_ok=True)
    if args.tier == "extended":
        run_test(args.output, "drift", ["-p", "audio-engine", "--test", "pipeline_harness"], ["production_resamplers_and_rings_correct_independent_clocks", "clock_rate_step_keeps_audio_continuous"], ignored=True)
        run_test(args.output, "dfn-quality", ["-p", "denoise-dfn", "--test", "speech_quality"], ["licensed_real_speech_is_preserved_and_noise_mixtures_improve"], ignored=True)
        run_test(args.output, "aec-dfn-quality", ["-p", "audio-engine", "--features", "aec-prototype", "--lib"], ["worker::speech_quality_tests::real_speech_survives_the_production_aec_then_dfn_chain"], ignored=True)
        run_test(args.output, "reset", ["-p", "audio-engine", "--test", "dfn_recovery"], ["production_model_recovers_from_a_single_input_overflow"], ignored=True)
    elif args.tier == "soak":
        env = os.environ | {"OPENKRISP_SOAK_SECONDS": str(args.seconds), "OPENKRISP_REQUIRE_RSS": "1"}
        log = run_test(args.output, "soak", ["-p", "audio-engine", "--test", "concurrent_soak"], ["concurrent_microphone_and_stereo_incoming_soak"], ignored=True, env=env, timeout=args.seconds + 900)
        report = json.loads(next(line.removeprefix("OPENKRISP_SOAK_RESULT ") for line in log.splitlines() if line.startswith("OPENKRISP_SOAK_RESULT ")))
        if report["seconds"] != args.seconds or report["joinedWorkers"] != 2 or report["baselineRssKiB"] is None:
            raise ValueError("soak evidence incomplete")
        (args.output / "soak.json").write_text(json.dumps(report, indent=2) + "\n")
    else:
        profiles = json.loads((ROOT / "tests/performance/profiles.json").read_text())
        if args.profile not in profiles:
            parser.error("performance requires an explicit known --profile; add profiles after calibration")
        profile = profiles[args.profile]
        host = hardware()
        if any(host[key] != profile[key] for key in ("system", "machine", "cpu")):
            raise ValueError(f"host does not match reference profile: {host}")
        env = os.environ | {"OPENKRISP_AEC_PACED": "1", "OPENKRISP_AEC_BURNERS": str(profile["burners"])}
        log = run_test(args.output, "performance", ["-p", "audio-engine", "--features", "aec-prototype", "--lib"], ["worker::tests::production_aec_dfn_graph_timing"], ignored=True, env=env)
        report = json.loads(next(line.removeprefix("OPENKRISP_PERF ") for line in log.splitlines() if line.startswith("OPENKRISP_PERF ")))
        validate_performance(report, profile)
        (args.output / "performance.json").write_text(json.dumps({"profile": args.profile, "hardware": host, "report": report}, indent=2) + "\n")

if __name__ == "__main__":
    main()

# Phase 2 Hardware Test Protocol

## Purpose

This protocol validates the parts of the headless microphone monitor that CI cannot prove: real
CPAL callback behavior, audible processed output, negotiated hardware formats, sustained queue
health, and release of operating-system audio resources. Running the protocol is required before
claiming the complete Phase 2 definition of done.

The monitor does not record audio or put PCM in its diagnostic output. It keeps microphone samples
inside the local Rust process and reports only low-rate counters and levels.

## Safety and prerequisites

- Use wired headphones, begin at a low output volume, and do not select loudspeakers. An open
  microphone-to-speaker route can create damaging acoustic feedback.
- Close applications that need exclusive access to either endpoint.
- Confirm microphone permission for the terminal or built binary on platforms that require it.
- Use the release build for timing and soak measurements.
- Record the operating system, hardware, power mode, model hash, and selected device IDs with the
  result. Device IDs are opaque strings and should be copied exactly from inventory output.

Build the monitor and enumerate devices:

```text
cargo build --release --locked -p audio-engine --bin openkrisp-monitor
cargo run --release --locked -p audio-engine --bin openkrisp-monitor -- --list-devices
```

## Short smoke test

Choose one input and one wired-headphone output from the inventory, then run for two minutes:

```text
./target/release/openkrisp-monitor \
  --input "INPUT_DEVICE_ID" \
  --output "OUTPUT_DEVICE_ID" \
  --duration-seconds 120
```

Verify all of the following:

1. The startup summary shows the intended devices, negotiated formats, and packaged model.
2. Speech is audible in the selected headphones and the input/output level fields react to speech.
3. Background noise is audibly different with normal processing versus a second `--bypass` run.
4. A temporary output interruption may increment `underruns`, but the process remains alive and
   resumes producing audio.
5. Ctrl-C prints the clean-stop message and the process exits promptly.
6. A subsequent run can immediately reopen both devices, showing that streams and the worker were
   released.

Stop immediately if feedback, clipping, or unsafe volume occurs. A subjective quality comparison
is evidence to record, not a replacement for the metric checks below.

## Two-hour baseline soak

Prefer input and output endpoints backed by the same physical interface for the Phase 2 baseline;
different devices are independent clocks and are an explicit Phase 3 drift-control scenario. Save
the one-second diagnostic stream:

```text
./target/release/openkrisp-monitor \
  --input "INPUT_DEVICE_ID" \
  --output "OUTPUT_DEVICE_ID" \
  --duration-seconds 7200 \
  | tee phase-2-soak.log
```

Keep the machine awake and in the intended production power mode. At the beginning, midpoint, and
end, also record process resident memory with the operating system's normal process monitor.

Each diagnostic line reports:

- current and high/low-water queue fills;
- input overruns, output underruns, and total discontinuities;
- last, p95, and p99 inference duration plus missed deadlines;
- model errors and resets;
- input/output stream issues, terminal issues, xruns, and callback counts;
- the current estimated fixed-plus-queued latency.

The baseline passes when:

- state remains `Running` for the full two hours;
- estimated latency and queue fill show no upward trend or sustained pinning at a capacity edge;
- resident memory has no sustained growth after startup;
- terminal stream issues, callback contract violations, and model errors remain zero;
- inference p99 remains below the recorded 10 ms model-frame deadline on the test machine;
- any isolated underrun or xrun is counted without a crash or deadlock;
- the timed stop or Ctrl-C exits promptly, and a second route reopens both devices.

If clock drift steadily moves output fill toward an edge, record the rate and endpoint pair rather
than relaxing the criterion. That result is input to Phase 3 adaptive drift work.

## Result record

Copy this table into the test report for each run:

| Field | Result |
|---|---|
| Date and tester | |
| OS and version | |
| CPU and power mode | |
| Input device ID / format | |
| Output device ID / format | |
| Model ID / SHA-256 | |
| Duration | |
| Initial / midpoint / final RSS | |
| Queue start / midpoint / end | |
| Inference p95 / p99 / missed | |
| Overruns / underruns / discontinuities | |
| Stream issues / xruns / terminal issues | |
| Model errors / resets | |
| Audible processed-versus-bypass result | |
| Clean stop and immediate reopen | |
| Pass / fail and notes | |

Run at minimum on the built-in endpoints and one USB interface for each supported operating system.
Treat Bluetooth as a separately recorded compatibility run because transport latency lies largely
outside this process.

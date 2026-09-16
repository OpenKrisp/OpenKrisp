# Speakerphone acceptance — E16-05 / E16-06

Speakerphone calls are a core OpenKrisp requirement. Acceptance means the other
participant hears the local speaker clearly without hearing their own playback
returned as echo, including while both people speak. It is not a promise of perfect
cancellation in every room, at every volume, or with every device.

**Current setting: enabled by default at the user's explicit request.** The
[desktop integration](AEC_DESKTOP_DEFAULT_IMPLEMENTATION.md) supersedes the earlier
decision to defer enablement. The native AEC passes the three original software
quality gates and expanded synthetic controls, but physical timing, processed-voice
listening and actual-call acceptance remain open. This document defines qualification
evidence; default enablement does not close any roadmap item or certify speakerphone use.

## Required software check

After installing the pinned native build tools from the prototype report, run:

```sh
python3 scripts/check-aec-quality.py
```

The command builds the actual `aec-webrtc` test executables, verifies that the
required quality tests exist, and runs **all** discovered native test targets with
ignored tests included. Build errors, missing required tests, test failures,
timeouts, skipped tests and empty test runs cannot report success. It continues
through failing test targets and stores logs, source hashes and `result.json` in a
new directory under `target/aec-acceptance/`. Exit 0 means only that these offline
software gates passed; exit 1 means they did not. The three previously ignored
quality gates now run in routine `cargo ci-test`; the dedicated check additionally
enforces the required inventory and compiled native alignment contracts.

Keep the existing engineering targets: echo-only ERLE >10 dB; local projection gain
in `[0.7, 1.5)` and correlation >0.8 against a paired, same-stage local-only baseline.
Score voiced and unvoiced segments separately. Measure echo reduction only in
far-end-only intervals. A quieter output during double talk is not evidence of
success: it may mean OpenKrisp removed the user's voice. These numerical floors are
necessary synthetic regressions, not sufficient room or intelligibility acceptance.

Any change to the AEC configuration, native dependency, reference placement,
resampling, fallback or DFN ordering requires this check, the affected worker and
callback regressions, and the normal workspace checks. Speakerphone qualification requires
the physical and application evidence below as well. Passing CI alone must never
be used as physical or conferencing acceptance.

## Acceptance matrix

| Condition | Required observation / evidence |
| --- | --- |
| Far-end speech only | Settle, then score echo separately from room noise. The receiving listener reports no intelligible returned speech or persistent echo. Include intermittent speech, silence and stereo/dual-mono playback. |
| Local speech only | Compare with AEC bypass through the same DFN path. Preserve sentence onsets, soft consonants, pauses and normal/quiet speech; no missing words or pumping. |
| Both people speaking | Preserve local words and consonants while controlling returned speech. Use quiet, normal and louder local speech relative to playback; record actual signal levels for offline fixtures rather than calling amplitude multipliers SNR. |
| Acoustic path and volume changes | Test delay-only, gain-only and combined changes in synthetic fixtures; retain the existing two-second recovery allowance and fixed seconds 8–10 scoring window. In the room, move the laptop/microphone and change playback volume; verify recovery and local-voice preservation, including the transition itself. |
| Reference loss and lifecycle | Test underrun silence, overflow, missing timestamps, Stop/restart, device removal and recovery. No old reference may become valid again. Verify observable unavailable/fallback state and renewed cancellation after fresh alignment. |
| Timing and clocks | Preserve the existing 44.1/48/96 kHz and independent-clock/drift tests. Run both routes concurrently with real AEC + DFN, with and without CPU load; require zero missed processing deadlines, xruns and stream/model errors in the accepted run. Record startup/fallback separately and final counters after shutdown. |
| Sustained use | Complete the outstanding two-hour concurrent hardware runs. A short clean run does not substitute for them. Record memory, queue bounds, drift, fallbacks and all final error/deadline counters. |
| Receiving application | Exercise an actual conferencing application's outgoing virtual microphone and incoming speaker path simultaneously. Listen at the receiving endpoint. Record application/device versions and settings, including its own AEC/AGC/noise suppression where controllable. Do not credit the conferencing application's AEC as proof that OpenKrisp passed. |

Start with the user's MacBook built-in speakers and microphone. Qualify other
supported device/platform combinations separately, including Linux, independent
USB device clocks and Bluetooth profile changes where applicable. Each accepted
combination needs its own evidence; an untested combination stays unqualified.

For repeatable listening comparisons, use the same far-end fixture and local
phrases with AEC bypassed, AEC enabled, and AEC + DFN enabled. Include local-only,
far-only and overlapping intervals. Record output volume, device rates/buffers,
room geometry and distance. Review the processed microphone at the receiving
endpoint, without feeding that monitor back through the speakers under test.
Any microphone recording needs the user's explicit agreement and a documented
retention/deletion choice; the current automated checks record no microphone PCM.

## Desktop integration requirements

The render reference must include the remote audio actually submitted to the
speakers. Unreferenced playback from another application is outside cancellation
coverage. The existing native application-source harness is useful evidence, but
does not establish the desktop conferencing flow.

On macOS, the same BlackHole bus cannot simultaneously serve as the incoming
loopback source and the outgoing virtual microphone without a feedback conflict.
Desktop integration must establish distinct buses or another explicit supported
route topology before a full-duplex speakerphone claim. Do not infer desktop
support from the native harness's workaround.

Desktop controls now distinguish denoise bypass, active AEC and AEC unavailable.
With default enablement, an invalid reference retains aligned microphone audio through
the DFN path and displays unavailable/waiting status; that audio is **not echo-protected**.
The off preference applies on the next microphone start and persists in profiles. Headphones
remain the documented fallback while any required acceptance evidence is missing.

## Evidence as of 2026-09-15

- The [quality fixes](AEC_QUALITY_FIXES.md) resolve stationary broadband
  preservation, weaker local speech and stereo delay-change recovery without
  lowering the existing targets. Fifteen native tests and seven compiled mixer
  contracts pass on macOS ARM64 and Linux x86_64. Expanded early local-speech,
  varied-speech, reflection, speaker-distortion and unequal-coupling checks pass.
  Twenty additional early-echo scoring windows were added after the Linux run
  and verified on macOS. [Current evidence](evidence/aec-quality-fixes-2026-09-15.json)
  retains the exact scope; [earlier failures](evidence/aec-speakerphone-gates-2026-09-15.json)
  remain historical evidence.
- Earlier paced AEC + DFN measurements included six missed deadlines in 1,000
  frames. Fresh runs of the current profile have zero misses with and without
  two CPU load threads; the earlier misses' cause has not been established or fixed.
- The [2026-09-11 hardware smoke test](AEC_LIVE_SMOKE_TEST.md) predates the explicit
  stereo-channel fix. It did not include processed-microphone listening or an
  actual conference. No new physical-device acceptance is claimed here.

Default desktop integration is documented [separately](AEC_DESKTOP_DEFAULT_IMPLEMENTATION.md).
The next acceptance work is processed-microphone listening with real local speech,
followed by repeatable room/path/volume comparisons and the actual conferencing flow.
The optional native harness can save a bounded local processed-output clip after
explicit recording agreement. Further native changes must preserve the whole matrix,
including false alignment during local-only/double-talk intervals.

# Virtual Cables evaluation — 2026-09-08

Decision: **do not replace AudioMirror with this revision yet**. Basic audio forwarding works,
but acceptance tests reproduce sample-order corruption, ignored device mute and incomplete
session shutdown. This changes the earlier assessment from a promising candidate to one that
needs fixes before integration. It does not establish that AudioMirror is production ready.

Subsequent product decision: [Windows support has been removed](WINDOWS_SUPPORT.md), along with
the adapters and candidate evaluation tools. The results below are historical evidence from the
completed evaluation; they do not describe an active integration or planned release work.

## Tested source and environment

- [Virtual Cables commit a93b8d72868ddbdc0aaf94076c6814f5097bdcbc](https://github.com/tarekwasfy01/Virtual-Cables/tree/a93b8d72868ddbdc0aaf94076c6814f5097bdcbc), application version 0.5.13, BSD-2-Clause.
- Host: macOS ARM64; Go 1.26.2. No Windows test environment was identified in this workspace.
- Only a copied upstream source tree was tested. No driver or installer was executed.
- Tests use the real upstream USB/IP connection handler, UAC1 control handling and ring buffer
  through an in-process connection. The Windows x64 application was cross-built, not executed.
- [Recorded results](evidence/virtual-cables-2026-09-08.json) retain the commit, test-source hash,
  commands, exit codes, per-test outcomes, failure messages and log hashes. Raw logs from this
  run were written to `/tmp/openkrisp-virtual-cables-results-2`. The evaluator has since been removed
  with the Windows-specific tooling; the recorded JSON preserves the results.

## Results

| Test | Result |
| --- | --- |
| Upstream suite, with race detector | 16 tests pass; no race detector report |
| Host and Windows x64 static analysis | `go vet` passes on both targets |
| Windows x64 application build | Pass; unsigned executable, not an installed driver test |
| Upstream 32-cable self-test | Pass |
| Stereo PCM through USB/IP request/reply handling | Pass, exact channel/sample preservation for sequential transfers |
| 32 concurrent independent cables | Pass, no cross-cable contamination in this test |
| Ten interface stop/restart cycles | Pass, old buffered PCM cleared and fresh PCM received |
| Buffer underrun and overrun | Pass, silence on empty buffer and newest aligned PCM retained on overflow |
| Queued playback | **Fail**, PCM order changes with multiple pending requests |
| Advertised device mute | **Fail**, both playback and capture feature units acknowledge mute but forward nonzero PCM |
| Cancel imported session | **Fail**, the connection handler remains blocked until the client closes |

The four passing and three failing acceptance rows are top-level test groups, not an inflated
count including the 32 cable subtests. The upstream streaming server initially has only 18.9%
statement coverage; passing that suite did not establish streaming correctness.

Queued playback failed **all 40 repeated subtest cases**: ten runs each of the 1 ms and 10 ms
request sizes with the race detector, and ten each without it. These are additional to the
initial acceptance run. All failures report differing PCM samples, not timeouts or race warnings.

## Reproduced failures

### Playback order

The test queues distinct stereo PCM blocks and verifies the complete captured byte sequence.
It uses 64 requests of 1 ms each and 16 requests of 10 ms each. Neither exceeds the configured
250 ms buffer, so overflow is not the cause. All playback completions are drained before the
single capture request checks the result.

The [upstream server](https://github.com/tarekwasfy01/Virtual-Cables/blob/a93b8d72868ddbdc0aaf94076c6814f5097bdcbc/src/internal/usbip/server.go)
launches a goroutine for each isochronous request. For playback, each goroutine writes its PCM
to the shared ring **before** waiting for its assigned completion time. A mutex makes each ring
write atomic, but does not preserve the order in which the requests arrived. The deadline test
upstream therefore passes while the actual audio-order test fails. A race detector need not flag
this ordering bug because memory access itself is synchronized.

Preserve per-endpoint processing order, including when timers are late or requests are canceled,
before treating this as suitable for live microphone audio. Native playback quality has not been
measured; audible glitches are a likely consequence of the observed sample reordering.

### Mute

Both feature units advertise master mute. A valid UAC1 SET_CUR request succeeds, but the
[device implementation](https://github.com/tarekwasfy01/Virtual-Cables/blob/a93b8d72868ddbdc0aaf94076c6814f5097bdcbc/src/internal/uac1/device.go)
only stores the mute value. Neither WritePlayback nor ReadCapture applies it to PCM. The test
continues to receive the original nonzero samples after setting mute on either feature unit.
This is a device-control failure; Windows software mute behavior has not been tested here.

### Shutdown

After a device import, the server clears the socket deadline and waits for more requests.
Canceling the parent context does not close that connection or interrupt the blocking read.
The test allows 300 ms for cancellation, then fails; explicitly closing the client lets the
handler exit. The application stop path cancels its server context and waits for the listener,
so that wait alone does not establish that imported sessions have been cleaned up.

Track and close owned imported connections, and wait for their handlers on shutdown. Process
exit will close sockets, but an in-process stop/restart needs its own complete cleanup.

## Limits of the historical evaluation

Driver installation/signature validation, actual usbaudio.sys/WASAPI/CPAL transfer, physical
microphone and receiving-app compatibility, latency, sleep/reboot/recovery, uninstall and
long-run stability were not tested. The source failures were enough to reject the evaluated
revision. Windows support was subsequently removed, so these are no longer release work items.

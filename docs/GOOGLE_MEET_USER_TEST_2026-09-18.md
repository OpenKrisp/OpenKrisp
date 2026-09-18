# Google Meet user test — reported 2026-09-18

The user reported successful manual testing in a Google Meet call with a friend
on **macOS only**. The user explicitly confirmed that Linux was not tested.
This is a qualitative user report; the assistant did not observe or instrument the
call. The report date does not establish the exact date or duration of the call.

## Scenarios reported

| Direction | Procedure | Reported result |
| --- | --- | --- |
| Outgoing microphone | The user played videos with background noise on a phone during the call to check whether the friend could hear that noise. | The user described the app as working well in their tests. |
| Incoming call audio | The friend played noisy videos on her phone; the user enabled incoming audio cleanup to check the other direction. | The user reported that cleanup worked in this direction as well. |
| Speakerphone / overlapping speech | The user used the Mac's built-in microphone and speakers. Both people spoke simultaneously. | The user confirmed that neither participant heard echo. |

This adds real conferencing-use evidence for outgoing and incoming noise reduction,
plus a successful subjective speakerphone check during overlapping speech. It satisfies
E14-06's basic requirement to test with at least one conferencing application. The
report does not quantify residual noise, speech intelligibility, echo reduction or latency,
or establish whether both processing routes ran simultaneously throughout the whole call.

## Setup details not yet reported

- Installed OpenKrisp build, OS/browser versions and selected Google Meet audio devices.
- Google Meet's own noise reduction, echo cancellation and gain settings; no paired
  OpenKrisp bypass comparison was described, so the contribution of each processor
  cannot be isolated from this report.
- Call duration and listening conditions such as output volume and room geometry.
- AEC status, diagnostic counters, CPU load, or device/volume/path transitions.

Phone-played noise tests background-noise suppression. The overlapping-speech check
additionally supports subjective speakerphone usability on the user's Mac. Google
Meet's own processing settings were not reported, so absence of perceived echo cannot
be attributed solely to OpenKrisp's AEC. No two-hour run or measured timing result is claimed.

E14-06 is complete for this user-reported macOS Google Meet check. Linux hardware
and conferencing remain untested; automated Linux checks do not establish that acceptance.
Other application/device combinations remain unqualified under the hardware matrix.
E16-05/E16-06's broader criteria, two-hour hardware stability, device recovery and
Bluetooth changes remain open. Additional setup details can extend this record without
replacing the existing automated and historical hardware evidence.

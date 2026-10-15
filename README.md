<img src="apps/desktop/src-tauri/icons/128x128@2x.png" alt="OpenKrisp app icon" width="128" height="128">

# OpenKrisp

Reduce background noise on both sides of a call.

OpenKrisp cleans your microphone and incoming call audio on your computer. Choose
your devices, turn on noise reduction, and select OpenKrisp in your call app.

- **Your microphone:** reduce the background noise other people hear from you.
- **Your speakers:** reduce background noise in the voices you hear on a call.
- **Local processing:** OpenKrisp processes audio on your device without uploading
  it to a cloud service.
- **Independent controls:** use either side on its own or turn on both.

**Experimental beta for macOS and Linux.** Device compatibility and real-call
quality are still being tested. Start with headphones; speakerphone use has not
yet been validated.

## Get OpenKrisp

Check [Releases](https://github.com/OpenKrisp/OpenKrisp/releases) for available
installers and the validation notes for each version. Downloads require access
to this repository while the beta is private.

The beta targets these platforms:

| Platform | Package | Installation guide |
| --- | --- | --- |
| macOS 11 or later on Apple Silicon | `.pkg`, including both virtual audio devices | [macOS setup](docs/COMMUNITY_RELEASE.md#installing-on-macos) |
| Debian 12 or Ubuntu 24.04 on x86-64 | `.deb` | [Linux setup](packaging/linux/INSTALL.md) |
| Fedora 44 on x86-64 | `.rpm` | [Linux setup](packaging/linux/INSTALL.md) |

The macOS beta installer is unsigned and the app is not notarized by Apple;
the installation guide explains the first-launch prompts. Linux release packages
are still awaiting validation and require an active PipeWire session. Windows
and Intel Mac installers are not provided.

## Set up your first call

Install OpenKrisp using the guide for your platform, then open the app. On macOS,
allow microphone access when prompted. You can set up either panel independently.

### Clean your microphone

1. Under **Your microphone**, choose the microphone you normally use.
2. Select **Turn on OpenKrisp** and wait for it to finish starting.
3. In your call app's audio settings, select **OpenKrisp Microphone** as the
   microphone or input device.

### Clean incoming voices

1. Under **Your speakers**, use **Play through** to choose your headphones.
2. Select **Turn on OpenKrisp** and wait for it to finish starting.
3. In your call app's audio settings, select **OpenKrisp Speaker** as the speaker
   or output device.

On Linux, the OpenKrisp devices appear in your call app after you turn on the
matching panel. If they are missing from the list, reopen the call app's audio
settings or restart the call app.

Keep OpenKrisp running during the call. It remembers your device choices, but
reopening the app does not start audio automatically.

## Everyday use

- **To stop processing:** switch your call app back to your usual microphone and
  headphones, then select **Turn off OpenKrisp** in the matching panel. Switching
  back first keeps the call from losing its audio input or output.
- **To change devices:** turn off the matching panel, choose the new device,
  then turn it on again.
- **For music:** select your usual headphones or speakers directly in the other
  app. OpenKrisp's noise reduction is designed for speech.
- **To keep it out of the way:** close the window. Audio keeps running; use the
  menu bar or system tray icon to show OpenKrisp again. Choose **Quit** there to
  exit the app.

## Need help?

Open **Help** in OpenKrisp for connection instructions and **Refresh devices**.

| Problem | What to try |
| --- | --- |
| Your microphone or headphones are missing | Reconnect the device, then select **Help → Refresh devices**. On macOS, check that OpenKrisp has microphone permission. |
| Your call app cannot find the OpenKrisp devices | On macOS, follow the installer and restart instructions. On Linux, turn on the matching panel first and check the PipeWire setup in the installation guide. |
| You cannot hear the call | Check **Play through**, make sure **Your speakers** is on, and confirm the call app is using **OpenKrisp Speaker**. |
| Other people cannot hear you | Check the microphone selected in OpenKrisp, make sure **Your microphone** is on, and confirm the call app is using **OpenKrisp Microphone**. |
| You hear echo | Use headphones while speakerphone support is being validated. |

If the problem persists, select **Help → Copy support details** and include the
report in an [issue](https://github.com/OpenKrisp/OpenKrisp/issues), along with your
OpenKrisp version, operating system, and steps to reproduce it. The support report
contains technical status without recordings or device names.

Updater-enabled releases check for new versions and download them automatically
once the repository is public. Updates install and restart after a 30-second
countdown when both audio routes are off; choose **Later** to postpone. See
[automatic updates](docs/AUTOMATIC_UPDATES.md).
Older versions require one manual upgrade. For removal, follow the
[macOS guide](docs/COMMUNITY_RELEASE.md#installing-on-macos) or
[Linux guide](packaging/linux/INSTALL.md#update-or-remove).

## Development

To build from source or contribute, start with [CONTRIBUTING.md](CONTRIBUTING.md).
It covers prerequisites, build commands, checks, and links to the architecture,
roadmap, and other developer documentation.

## License

OpenKrisp is source available under the [PolyForm Perimeter License 1.0.0](LICENSE),
with the scope described in [NOTICE](NOTICE). The license restricts providing
competing products, including free competing forks. Third-party components retain
their own licenses; see [licensing details](docs/LICENSING.md).

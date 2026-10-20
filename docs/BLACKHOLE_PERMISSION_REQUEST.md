# Draft BlackHole permission request

Historical draft from before the 2026-10-20 GPLv3 transition. Retained for the
earlier PolyForm licensing record; this is not the current distribution plan.
See [current licensing](LICENSING.md).

Status: **unsent draft, not an agreement or grant of permission**.
Fill in the sender's legal name and project URL before sending. No fees,
obligations, or permissions have been accepted by either party.

To: Existential Audio / BlackHole maintainers
Contact listed upstream: devinroth@existential.audio
Subject: Permission request for BlackHole integration in OpenKrisp

Hello BlackHole team,

I maintain OpenKrisp, a desktop application for local, real-time microphone and
incoming call noise reduction on macOS and Linux. I would like to request written
permission to distribute a BlackHole integration while licensing OpenKrisp's
original code under PolyForm Perimeter 1.0.0.

Our goal is to make the source available for inspection and improvement while
restricting reuse of our code in competing products. We understand that this is
not a GPL license, and your published integration guidance calls for a separate
license for non-GPL projects. We are seeking agreement before distributing the
combined installer under this arrangement.

The macOS integration currently builds pinned BlackHole 0.7.1 source into two
HAL audio drivers named OpenKrisp Microphone (2 channels) and OpenKrisp Speaker
(16 channels, with OpenKrisp using channels 1-2). We customize names, identifiers,
factory identities, channel counts, and the icon through build settings without
editing the upstream C source. The drivers and application are separate bundles
delivered in a single installer. Existing official BlackHole drivers are left
untouched.

The installer includes your copyright and license notices, the pinned source
archive, and the build inputs used for these drivers. We would preserve the
attribution and source provisions required by an agreement and would not claim
your endorsement or apply our PolyForm restrictions to your GPL-licensed code.

Would you consider a no-cost written permission or licensing arrangement for
this integration? If so, could you share your preferred terms? We would like
to establish:

- The permitted building, customization, naming, and distribution of these
  drivers with OpenKrisp under its separate PolyForm Perimeter license.
- Whether the permission covers public source/build instructions, release
  downloads, updates, and redistribution by OpenKrisp users, and any conditions
  on those activities.
- Whether recipients may rebuild and use the drivers with permitted OpenKrisp
  modifications, and what rights and notices apply to each component.
- The covered BlackHole versions, duration, treatment of already distributed
  copies, and any attribution, source, branding, or fee requirements.

We can provide our repository and packaging scripts for review. We are happy
to work from your standard agreement and resolve the exact scope together.

Thank you,

[Sender's legal name / legal entity]
[Contact email]
[OpenKrisp repository or project URL]

---

Reference: [BlackHole integration guidance](https://github.com/ExistentialAudio/BlackHole#can-i-integrate-blackhole-into-my-app).
The above requests terms; it does not assert that an exception has been granted.

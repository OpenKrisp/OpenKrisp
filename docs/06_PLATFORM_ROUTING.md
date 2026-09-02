# Platform Routing

## 1. Scope

CPAL provides access to audio input and output streams. It does not, by itself, guarantee a cross-platform way to:

- Expose processed audio as a virtual microphone.
- Capture all system output.
- Create a virtual speaker.
- Prevent recursive capture loops.
- Install and sign native virtual-audio drivers.

These concerns belong behind a platform-routing abstraction.

## 2. Route types

```rust
pub enum SourceEndpoint {
    PhysicalInput(DeviceId),
    SystemLoopback(LoopbackEndpointId),
    ApplicationStream(ApplicationStreamId),
    VirtualRouteInput(VirtualEndpointId),
}

pub enum DestinationEndpoint {
    PhysicalOutput(DeviceId),
    VirtualMicrophone(VirtualEndpointId),
    ApplicationSink(ApplicationSinkId),
}
```

The first release needs only `PhysicalInput -> PhysicalOutput`. Other variants can exist in the domain model but should not be marked supported until implemented and tested.

## 3. Platform abstraction

```rust
pub trait PlatformAudioRouter {
    fn capabilities(&self) -> RoutingCapabilities;

    fn discover_endpoints(
        &self,
    ) -> Result<Vec<PlatformEndpoint>, RoutingError>;

    fn prepare_route(
        &mut self,
        request: &PlatformRouteRequest,
    ) -> Result<PreparedPlatformRoute, RoutingError>;

    fn release_route(
        &mut self,
        route_id: &RouteId,
    ) -> Result<(), RoutingError>;

    fn validate_no_feedback_loop(
        &self,
        request: &PlatformRouteRequest,
    ) -> Result<(), RoutingError>;
}
```

`PreparedPlatformRoute` should provide endpoint identifiers and any cleanup token needed by the supervisor. It should not expose raw operating-system handles through Tauri DTOs.

## 4. Capability reporting

```rust
pub struct RoutingCapabilities {
    pub physical_input: bool,
    pub physical_output: bool,
    pub system_loopback: bool,
    pub virtual_microphone: bool,
    pub virtual_speaker: bool,
    pub requires_external_driver: bool,
    pub supports_render_reference: bool,
}
```

The UI must use capability reporting rather than assuming feature parity across platforms.

## 5. macOS strategy

### MVP

- Use CPAL/CoreAudio for physical input and output.
- Ask for microphone permission through the normal application flow.
- Implement microphone monitoring with headphones.

### Virtual microphone

Practical staged approach:

1. Detect a supported existing virtual audio endpoint.
2. Route cleaned audio to that endpoint.
3. Show instructions for selecting it in conferencing applications.
4. Later evaluate a bundled signed CoreAudio virtual device as a separate deliverable.

### System-output capture

- Prefer an operating-system-supported loopback path when available and validated through CPAL or a platform-specific adapter.
- Otherwise use a virtual output device that applications select as their output.
- Play only the processed signal to the physical destination.

### Risks

- Microphone privacy permission.
- Audio-device identifiers changing after reinstall or OS update.
- Aggregate/multi-output device complexity.
- Driver signing and installer requirements.
- Feedback if the processed physical output is included in the capture source.

## 6. Windows strategy

### MVP

- Use CPAL's WASAPI backend for physical microphone and output streams.
- Validate shared-mode format negotiation across common devices.

### System loopback

- Use WASAPI loopback where supported, either through CPAL capabilities at the pinned revision or a narrowly scoped Windows adapter.
- Keep backend-specific code isolated from the engine.

### Virtual microphone

- Integrate with an existing virtual endpoint first.
- Treat a production custom virtual audio driver as a separate signed installer/driver project.

### Risks

- Shared versus exclusive mode behavior.
- Device role/default changes.
- Driver signing.
- Format conversion imposed by the Windows audio engine.
- Endpoint invalidation during sleep, USB reconnect, or Bluetooth profile switching.

## 7. Linux strategy

Prefer PipeWire on modern desktop systems.

Potential architecture:

```text
Physical microphone -> application input
Application cleaned output -> PipeWire virtual source
System/application audio -> PipeWire capture node -> application -> physical sink
```

A PipeWire-native routing helper may complement CPAL when virtual nodes or graph links are required. Keep this helper behind `PlatformAudioRouter`.

Support policy should clearly state whether PulseAudio compatibility is required or PipeWire is the baseline.

### Risks

- Distribution and desktop-environment differences.
- PipeWire versus PulseAudio sessions.
- Permissions and sandbox packaging.
- Dynamic graph changes.

## 8. Feedback-loop prevention

System-output processing can create recursion:

```text
captured system output -> process -> physical output -> captured again -> ...
```

The router must validate the graph before starting:

- Source and destination must not resolve to the same recursive virtual path.
- The processed destination should be excluded from loopback capture where the platform supports exclusion.
- Route metadata should track physical and virtual endpoint ancestry.
- Detect rapid gain escalation or repeated identical blocks as a secondary safety signal, not the primary prevention mechanism.
- Stop or bypass immediately when a confirmed loop is detected.

## 9. Acoustic echo cancellation

DeepFilterNet performs noise reduction and should not be treated as a complete acoustic echo canceller.

For open-speaker full duplex:

```text
render signal before speaker output --------------------------┐
                                                               v
physical microphone -> acoustic echo canceller -> DeepFilterNet -> virtual microphone
```

The platform and engine should preserve a render-reference hook now, even if the AEC module is added later:

```rust
pub trait RenderReferenceSink {
    fn push_render_frames(
        &mut self,
        timestamp: AudioTimestamp,
        frames: &[f32],
    );
}
```

Headphones should be the recommended configuration until AEC is implemented and validated.

## 10. Feature staging

### Stage 1

Physical microphone to physical headphones. No virtual routing.

### Stage 2

Cleaned microphone to a user-installed virtual audio endpoint.

### Stage 3

Application-owned incoming PCM to physical output.

### Stage 4

System loopback capture through platform adapters.

### Stage 5

Custom virtual endpoints, installer integration, and optional AEC.

Do not block the core engine MVP on custom driver development.

## 11. Routing acceptance criteria

A platform-routing feature is complete only when:

- The endpoint is discovered reliably after restart.
- Start/stop does not leak a route or graph connection.
- The route survives common default-device changes or fails with a clear recovery path.
- Feedback-loop validation is automated where possible.
- The UI reports installation or permission prerequisites.
- A test matrix covers at least one built-in device, one USB device, and one virtual endpoint on the target platform.

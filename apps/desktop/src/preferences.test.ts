import { describe, expect, it } from "vitest";
import { defaultSettings, initialSettings, preferenceChanges, sameSettings } from "./preferences";
import { snapshot } from "./testFixtures";
import type { AudioDevice } from "./api";

const device: AudioDevice = { id: "physical:duplex", name: "Device", host: "coreaudio", kind: "physical",
  supportsInput: true, supportsOutput: true, isDefaultInput: true, isDefaultOutput: true,
  manufacturer: null, driver: null, defaultInputFormat: null, defaultOutputFormat: null };
const endpoint = { id: "macos:openkrisp-microphone", name: "OpenKrisp Microphone", availability: "missing" as const,
  canCreate: false, playbackDeviceId: null, captureDeviceId: null, ancestry: { recursivePaths: [] as string[], complete: true }, instructions: [], setupUrl: "" };

describe("initial device preferences", () => {
  it("selects the two dedicated virtual roles even when setup is needed", () => {
    const inventory = { ...snapshot, inventory: { host: "coreaudio", devices: [device] },
      routing: { ...snapshot.routing, endpoints: [endpoint], loopbackEndpoints: [{ ...endpoint, id: "macos:openkrisp-speaker" }] } };
    expect(initialSettings(defaultSettings(), inventory)).toMatchObject({
      microphone: { inputId: device.id, outputId: "macos:openkrisp-microphone", echoCancellation: true },
      incoming: { inputId: "macos:openkrisp-speaker", outputId: device.id, echoCancellation: false },
    });
  });

  it("never follows virtual, aggregate, unknown or moving PipeWire default devices", () => {
    for (const bad of [{ ...device, kind: "virtual" as const }, { ...device, kind: "aggregate" as const },
      { ...device, kind: "unknown" as const }, { ...device, id: "pipewire:input_default" }, { ...device, id: "pipewire:output_default" }]) {
      const resolved = initialSettings(defaultSettings(), { ...snapshot, inventory: { host: "pipewire", devices: [bad] } });
      expect(resolved.microphone.inputId).toBe("");
      expect(resolved.incoming.outputId).toBe("");
    }
  });

  it("excludes endpoint ancestry even if an inventory device is misclassified physical", () => {
    const resolved = initialSettings(defaultSettings(), { ...snapshot, inventory: { host: "coreaudio", devices: [device] },
      routing: { ...snapshot.routing, endpoints: [{ ...endpoint, ancestry: { complete: true, recursivePaths: [device.id] } }] } });
    expect(resolved.microphone.inputId).toBe("");
    expect(resolved.incoming.outputId).toBe("");
  });

  it("does not guess an endpoint when discovery returns multiple candidates", () => {
    const resolved = initialSettings(defaultSettings(), { ...snapshot, routing: { ...snapshot.routing,
      endpoints: [endpoint, { ...endpoint, id: "another" }], loopbackEndpoints: [endpoint, { ...endpoint, id: "other" }] } });
    expect(resolved.microphone.outputId).toBe("");
    expect(resolved.incoming.inputId).toBe("");
  });

  it("distinguishes an invalid numeric draft from an unlimited reduction preference", () => {
    const saved = defaultSettings();
    for (const attenuationLimitDb of [NaN, Infinity, -Infinity]) {
      const invalid = structuredClone(saved);
      invalid.microphone.denoise.attenuationLimitDb = attenuationLimitDb;
      expect(sameSettings(invalid, saved)).toBe(false);
      expect(preferenceChanges(saved.microphone, invalid.microphone)).toEqual({ denoise: invalid.microphone.denoise });
    }
  });

  it("compares saved settings independently of property order", () => {
    const saved = defaultSettings();
    const reordered = { incoming: saved.incoming, microphone: {
      denoise: { postFilterEnabled: false, attenuationLimitDb: null, enabled: true },
      echoCancellation: true, mode: saved.microphone.mode, targetLatencyMs: 40,
      destinationKind: saved.microphone.destinationKind, outputId: "", inputId: "",
    } };
    expect(sameSettings(saved, reordered)).toBe(true);
    expect(preferenceChanges(saved.microphone, reordered.microphone)).toEqual({});
  });

  it("accepts snapshot changes only for fields the user has not edited", () => {
    const previous = defaultSettings().microphone;
    previous.denoise.attenuationLimitDb = 0.1;
    const next = { ...previous, inputId: "snapshot:mic", outputId: "snapshot:output",
      denoise: { ...previous.denoise, attenuationLimitDb: 0.10000000149 } };
    const current = { ...previous, inputId: "user:mic" };
    expect(preferenceChanges(previous, next, current)).toEqual({ outputId: "snapshot:output" });
    expect(preferenceChanges(previous, current)).toEqual({ inputId: "user:mic" });
  });

  it("retires technical preferences and bypass without losing physical devices", () => {
    const settings = defaultSettings();
    settings.microphone = { ...settings.microphone, inputId: "missing:mic", outputId: "old:monitor", destinationKind: "physical_output",
      echoCancellation: false, targetLatencyMs: 120, denoise: { enabled: false, attenuationLimitDb: 3, postFilterEnabled: true } };
    settings.incoming = { ...settings.incoming, inputId: "old:bus", outputId: "missing:headphones", mode: "aligned_bypass", targetLatencyMs: 200 };
    const next = initialSettings(settings, { ...snapshot, routing: { ...snapshot.routing,
      endpoints: [endpoint], loopbackEndpoints: [{ ...endpoint, id: "macos:openkrisp-speaker" }] } });
    expect(next.microphone).toEqual({ ...defaultSettings().microphone, inputId: "missing:mic", outputId: endpoint.id });
    expect(next.incoming).toEqual({ ...defaultSettings().incoming, inputId: "macos:openkrisp-speaker", outputId: "missing:headphones", mode: "speech_denoise" });
    expect(initialSettings(next, { ...snapshot, routing: { ...snapshot.routing,
      endpoints: [endpoint], loopbackEndpoints: [{ ...endpoint, id: "macos:openkrisp-speaker" }] } })).toEqual(next);
  });
});

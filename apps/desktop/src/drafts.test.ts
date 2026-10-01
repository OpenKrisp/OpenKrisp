import { defaultSettings, initialSettings, preferencesFromDraft } from "./preferences";
import { describe, expect, it } from "vitest";
import { defaultDraft, denoiseEqual, denoiseError, draftFromRoute, draftSettingsError, requestFromDraft } from "./drafts";
import { route, snapshot } from "./testFixtures";

describe("route drafts and saved preferences", () => {
  it("preserves an AEC opt-out through saved settings, requests and route restoration", () => {
    const draft = { ...defaultDraft("microphone"), echoCancellation: false };
    expect(defaultDraft("microphone").echoCancellation).toBe(true);
    expect(defaultDraft("incoming").echoCancellation).toBe(false);
    expect(preferencesFromDraft(draft).echoCancellation).toBe(false);
    expect(requestFromDraft(draft).echoCancellation).toBe(false);
    expect(draftFromRoute({ ...route, config: { ...route.config, echoCancellation: false } })?.echoCancellation).toBe(false);
    expect(requestFromDraft({ ...defaultDraft("incoming"), echoCancellation: true }).echoCancellation).toBe(false);
  });
  it("preserves the physical output while retiring saved tuning and bypass in the desktop", () => {
    const draft = { ...defaultDraft("incoming"), inputId: "loop:explicit", outputId: "output:missing", targetLatencyMs: 65,
      denoise: { enabled: false, attenuationLimitDb: 18.5, postFilterEnabled: true } };
    expect(initialSettings({ ...defaultSettings(), incoming: preferencesFromDraft(draft) }, snapshot).incoming).toEqual({ ...preferencesFromDraft(defaultDraft("incoming")), outputId: "output:missing", mode: "speech_denoise" });
    expect(requestFromDraft(draft)).toMatchObject({ sourceKind: "system_loopback", destinationKind: "physical_output",
      mode: "aligned_bypass", sourceDeviceId: "loop:explicit", destinationDeviceId: "output:missing", targetLatencyMs: 65, denoise: draft.denoise });
  });
  it("keeps physical microphone choices while requiring discovered call devices", () => {
    const draft = { ...defaultDraft("microphone"), inputId: "mic:explicit", outputId: "virtual:endpoint" };
    expect(initialSettings({ ...defaultSettings(), microphone: preferencesFromDraft(draft) }, snapshot).microphone).toEqual({ ...preferencesFromDraft(draft), outputId: "" });
    expect(requestFromDraft(draft).destinationKind).toBe("virtual_microphone");
  });
  it("resolves only unset physical preferences and leaves named missing preferences intact", () => {
    const inventory = structuredClone(snapshot);
    const device = { name: "Default device", host: "coreaudio", kind: "physical" as const, manufacturer: null, driver: null,
      supportsInput: false, supportsOutput: false, isDefaultInput: false, isDefaultOutput: false,
      defaultInputFormat: null, defaultOutputFormat: null };
    inventory.inventory.devices = [{ ...device, id: "input:default", isDefaultInput: true, supportsInput: true },
      { ...device, id: "output:default", isDefaultOutput: true, supportsOutput: true }];
    const preferences = defaultSettings();
    preferences.microphone.destinationKind = "physical_output";
    expect(initialSettings(preferences, inventory).microphone).toMatchObject({ inputId: "input:default", outputId: "", destinationKind: "virtual_microphone" });
    expect(initialSettings(preferences, inventory).incoming.outputId).toBe("output:default");
    preferences.microphone.inputId = "missing:input";
    expect(initialSettings(preferences, inventory).microphone.inputId).toBe("missing:input");
    expect(initialSettings(defaultSettings(), snapshot).microphone).toMatchObject({ inputId: "", outputId: "" });
  });
  it("retains authoritative denoise, mode and latency when restoring a route", () => {
    expect(draftFromRoute(route)).toMatchObject({ mode: route.config.processingMode, targetLatencyMs: 40, denoise: route.config.denoise });
    expect(draftFromRoute({ ...route, config: { ...route.config, source: { applicationStream: "native" } } })).toBeNull();
  });
  it("rejects invalid latency, non-finite/f32-overflow settings", () => {
    for (const value of [0, 1001, NaN, Infinity, 2.5]) expect(draftSettingsError({ ...defaultDraft("microphone"), targetLatencyMs: value })).not.toBeNull();
    for (const value of [-1, NaN, Infinity, 1e50]) expect(denoiseError({ ...route.config.denoise, attenuationLimitDb: value })).not.toBeNull();
    for (const value of [null, 0, 12.5]) expect(denoiseError({ ...route.config.denoise, attenuationLimitDb: value })).toBeNull();
  });
  it("compares settings at Rust f32 precision when confirming a snapshot", () => {
    const requested = { ...route.config.denoise, attenuationLimitDb: 0.10000000149 };
    expect(denoiseEqual({ ...requested, attenuationLimitDb: 0.1 }, requested)).toBe(true);
    expect(denoiseEqual({ ...requested, attenuationLimitDb: 0.2 }, requested)).toBe(false);
    expect(denoiseEqual({ ...requested, attenuationLimitDb: null }, requested)).toBe(false);
    expect(denoiseEqual({ ...requested, attenuationLimitDb: -0 }, { ...requested, attenuationLimitDb: 0 })).toBe(true);
  });
});

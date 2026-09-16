import { describe, expect, it } from "vitest";
import { defaultDraft, denoiseEqual, denoiseError, draftFromProfile, draftFromRoute, draftSettingsError, profileFromDraft, profileNameError, requestFromDraft } from "./drafts";
import { route, snapshot } from "./testFixtures";

describe("route drafts and profile preferences", () => {
  it("preserves an AEC opt-out through profiles, requests and route restoration", () => {
    const draft = { ...defaultDraft("microphone"), echoCancellation: false };
    expect(defaultDraft("microphone").echoCancellation).toBe(true);
    expect(defaultDraft("incoming").echoCancellation).toBe(false);
    expect(draftFromProfile(profileFromDraft(draft, "quiet", "Headphones"), snapshot).echoCancellation).toBe(false);
    expect(requestFromDraft(draft).echoCancellation).toBe(false);
    expect(draftFromRoute({ ...route, config: { ...route.config, echoCancellation: false } })?.echoCancellation).toBe(false);
    expect(requestFromDraft({ ...defaultDraft("incoming"), echoCancellation: true }).echoCancellation).toBe(false);
  });
  it("round-trips incoming mode, endpoints and settings without defaulting to denoise", () => {
    const draft = { ...defaultDraft("incoming"), inputId: "loop:explicit", outputId: "output:missing", targetLatencyMs: 65,
      denoise: { enabled: false, attenuationLimitDb: 18.5, postFilterEnabled: true } };
    const profile = profileFromDraft(draft, "incoming", "My call");
    expect(draftFromProfile(profile, snapshot)).toEqual(draft);
    expect(requestFromDraft(draft)).toMatchObject({ sourceKind: "system_loopback", destinationKind: "physical_output",
      mode: "aligned_bypass", sourceDeviceId: "loop:explicit", destinationDeviceId: "output:missing", targetLatencyMs: 65, denoise: draft.denoise });
  });
  it("keeps a virtual microphone endpoint explicit through profiles", () => {
    const draft = { ...defaultDraft("microphone"), inputId: "mic:explicit", outputId: "virtual:endpoint" };
    const profile = profileFromDraft(draft, "virtual", "Virtual");
    expect(draftFromProfile(profile, snapshot)).toEqual(draft);
    expect(requestFromDraft(draft).destinationKind).toBe("virtual_microphone");
  });
  it("resolves only explicitly saved defaults and leaves named missing preferences intact", () => {
    const inventory = structuredClone(snapshot);
    const device = { name: "Default device", host: "coreaudio", kind: "physical" as const, manufacturer: null, driver: null,
      supportsInput: false, supportsOutput: false, isDefaultInput: false, isDefaultOutput: false,
      defaultInputFormat: null, defaultOutputFormat: null };
    inventory.inventory.devices = [{ ...device, id: "input:default", isDefaultInput: true, supportsInput: true },
      { ...device, id: "output:default", isDefaultOutput: true, supportsOutput: true }];
    const profile = profileFromDraft({ ...defaultDraft("microphone"), destinationKind: "physical_output", inputId: "default", outputId: "default" }, "default", "Default");
    expect(draftFromProfile(profile, inventory)).toMatchObject({ inputId: "input:default", outputId: "output:default" });
    expect(draftFromProfile({ ...profile, sourceDevicePreference: "missing:input" }, inventory).inputId).toBe("missing:input");
    expect(draftFromProfile(profile, snapshot)).toMatchObject({ inputId: "", outputId: "" });
  });
  it("retains authoritative denoise, mode and latency when restoring a route", () => {
    expect(draftFromRoute(route)).toMatchObject({ mode: route.config.processingMode, targetLatencyMs: 40, denoise: route.config.denoise });
    expect(draftFromRoute({ ...route, config: { ...route.config, source: { applicationStream: "native" } } })).toBeNull();
  });
  it("rejects invalid latency, non-finite/f32-overflow settings and invalid profile names", () => {
    for (const value of [0, 1001, NaN, Infinity, 2.5]) expect(draftSettingsError({ ...defaultDraft("microphone"), targetLatencyMs: value })).not.toBeNull();
    for (const value of [-1, NaN, Infinity, 1e50]) expect(denoiseError({ ...route.config.denoise, attenuationLimitDb: value })).not.toBeNull();
    for (const value of [null, 0, 12.5]) expect(denoiseError({ ...route.config.denoise, attenuationLimitDb: value })).toBeNull();
    for (const name of [" ", "a\nb", "🎧".repeat(33)]) expect(profileNameError(name)).not.toBeNull();
    expect(profileNameError("🎧 Office")).toBeNull();
  });
  it("compares settings at Rust f32 precision when confirming a snapshot", () => {
    const requested = { ...route.config.denoise, attenuationLimitDb: 0.10000000149 };
    expect(denoiseEqual({ ...requested, attenuationLimitDb: 0.1 }, requested)).toBe(true);
    expect(denoiseEqual({ ...requested, attenuationLimitDb: 0.2 }, requested)).toBe(false);
    expect(denoiseEqual({ ...requested, attenuationLimitDb: null }, requested)).toBe(false);
  });
});

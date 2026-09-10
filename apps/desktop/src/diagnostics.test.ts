import { expect, it } from "vitest";
import { diagnosticReport } from "./diagnostics";
import { route, snapshot } from "./testFixtures";

it("exports useful counters and formats for both routes without any private strings or unknown fields", () => {
  const secret = "PRIVATE /Users/person/recording email@example.test DEVICE-123";
  const privateRoute = structuredClone(route);
  privateRoute.config.id = secret;
  privateRoute.config.source = { physicalInput: secret };
  privateRoute.config.destination = { physicalOutput: secret };
  privateRoute.inputDevice = { name: secret, id: secret } as typeof privateRoute.inputDevice;
  privateRoute.inputFormat = { sampleRateHz: 48000, channels: 1, sampleFormat: "f32", bufferSizeFrames: 512 };
  privateRoute.outputFormat = { ...privateRoute.inputFormat, sampleFormat: secret };
  privateRoute.fault = { code: "device_not_found", message: secret, details: secret, suggestedAction: secret };
  privateRoute.inputStream.lastIssue = secret;
  privateRoute.inputStream.lastTerminalIssue = "DeviceNotAvailable";
  privateRoute.metrics.missedDeadlines = 7;
  Object.assign(privateRoute.metrics, { futurePrivateField: secret });
  const incoming = { ...privateRoute, config: { ...privateRoute.config, source: { systemLoopback: secret } } };
  const text = diagnosticReport({ ...snapshot, inventory: { host: secret, devices: [] }, routes: [privateRoute, incoming], lastFault: privateRoute.fault }, false);
  expect(text).not.toContain(secret);
  expect(text).not.toContain("futurePrivateField");
  const report = JSON.parse(text);
  expect(report.connected).toBe(false);
  expect(report.routes.map((item: { role: string }) => item.role)).toEqual(["Microphone", "Incoming audio"]);
  expect(report.routes[0].metrics.missedDeadlines).toBe(7);
  expect(report.routes[0].inputFormat).toEqual(privateRoute.inputFormat);
  expect(report.routes[0].inputStream.lastTerminalIssue).toBe("DeviceNotAvailable");
  expect(report.routes[0].faultCode).toBe("device_not_found");
  expect(report.latencyDescription).toContain("not measured end-to-end");
});

it("bounds reports to the two route slots and normalizes invalid numeric observations", () => {
  const invalid = { ...route, metrics: { ...route.metrics, inputPeak: NaN, outputPeak: Infinity },
    model: { id: "private-model-path", channels: 1, sampleRateHz: 48000, frameSamplesPerChannel: 480, algorithmicLatencySamples: 480 } };
  const report = JSON.parse(diagnosticReport({ ...snapshot, routes: [invalid, invalid, invalid] }, true));
  expect(report.routes).toHaveLength(2);
  expect(report.routes[0].model.id).toBe("other");
  expect(report.routes[0].metrics.inputPeak).toBeNull();
  expect(report.routes[0].metrics.outputPeak).toBeNull();
  expect(JSON.parse(diagnosticReport(snapshot, true)).routes).toEqual([]);
});

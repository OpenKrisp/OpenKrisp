import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AudioLevel } from "./AudioControls";

function meter(value: number, active = true) {
  const markup = renderToStaticMarkup(<AudioLevel value={value} active={active} label="Input level" />);
  return { level: Number(markup.match(/ value="([^"]+)"/)![1]), markup };
}

describe("audio level display", () => {
  it.each([
    [0.001, 0], // -60 dBFS: display floor
    [0.01, 1 / 3], // -40 dBFS: quiet speech remains visible
    [0.1, 2 / 3], // -20 dBFS
    [1, 1], // 0 dBFS: full scale
  ])("maps a peak of %s onto a logarithmic meter", (peak, expected) => {
    expect(meter(peak).level).toBeCloseTo(expected, 6);
  });

  it.each([0, -1, 0.0001, NaN, Infinity, -Infinity])("keeps silence and invalid peak %s empty", (peak) => {
    const { level, markup } = meter(peak);
    expect(level).toBe(0);
    expect(markup).toContain('aria-valuetext="Quiet"');
  });

  it("caps overloads and clears the bar immediately when inactive", () => {
    expect(meter(2).level).toBe(1);
    const { level, markup } = meter(1, false);
    expect(level).toBe(0);
    expect(markup).toContain('aria-valuetext="Off"');
  });

  it("describes the measured level rather than a linear amplitude percentage", () => {
    expect(meter(0.01).markup).toContain('aria-valuetext="-40 dBFS"');
  });
});

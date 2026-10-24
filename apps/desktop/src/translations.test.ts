import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { catalogs, isLanguagePreference, languageOptions, localizedError, pluralMessageId, resolveLanguage, translator } from "./translations";
import { en, type MessageId } from "./locales/en";

describe("language resolution", () => {
  it.each([
    [["es-AR", "en-US"], "es"], [["es-ES"], "es"], [["ES_mx"], "es"],
    [["en-GB", "es"], "en"], [["fr-FR", "es-MX"], "fr"], [["fr", "de"], "fr"],
    [["fr-CA"], "fr"], [["de-AT"], "de"], [["de-CH", "en"], "de"],
    [["pt-BR"], "pt-BR"], [["PT_br"], "pt-BR"], [["pt"], "pt-BR"], [["pt-PT", "en"], "pt-BR"],
    [["ja", "de-DE"], "de"], [["ja", "nl"], "en"], [[], "en"],
  ] as const)("resolves the first supported system language in %j", (languages, expected) => {
    expect(resolveLanguage("system", languages)).toBe(expected);
  });

  it("gives an explicit choice precedence over the system", () => {
    expect(resolveLanguage("en", ["es-AR"])).toBe("en");
    expect(resolveLanguage("es", ["en-US"])).toBe("es");
    for (const { id } of languageOptions) {
      expect(isLanguagePreference(id)).toBe(true);
      expect(resolveLanguage(id, ["ja"])).toBe(id);
    }
    for (const invalid of [null, "", "es-AR", "fr-CA", "pt", "pt-PT", "it", "__proto__", "constructor", {}, true]) expect(isLanguagePreference(invalid)).toBe(false);
  });
});

describe("message catalogs", () => {
  it("requires complete nonempty catalogs with matching interpolation parameters", () => {
    const placeholders = (value: string) => [...value.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();
    for (const catalog of Object.values(catalogs)) {
      expect(Object.keys(catalog).sort()).toEqual(Object.keys(en).sort());
      for (const id of Object.keys(en) as MessageId[]) {
        expect(catalog[id].trim(), id).not.toBe("");
        expect(placeholders(catalog[id]), id).toEqual(placeholders(en[id]));
      }
    }
  });

  it("interpolates versions, names and numbers without interpreting them as replacement patterns", () => {
    expect(translator("es")("updates.ready", { version: "2.0.0-beta.1" })).toBe("OpenKrisp 2.0.0-beta.1 está listo para instalar.");
    expect(translator("es")("validation.setupSpeaker", { device: "$& {device}" })).toContain("$& {device}");
    expect(translator("en")("updates.countdownOne", { seconds: 1 })).toContain("1 second.");
  });

  it("covers every stable native error code and retains native diagnostics", () => {
    const protocol = readFileSync(new URL("../../../crates/audio-protocol/src/error.rs", import.meta.url), "utf8");
    const codes = [...protocol.matchAll(/Self::\w+ => "(\w+)"/g)].map((match) => match[1]!);
    expect(codes.length).toBeGreaterThan(15);
    for (const { id } of languageOptions) {
      for (const code of codes) {
        const error = { code, message: "Native diagnostic", details: "Technical details", suggestedAction: "Native recovery" };
        expect(localizedError(error, "en")).toBe(error);
        if (id === "en") continue;
        expect(localizedError(error, id).message, `${id}/${code}`).not.toBe("Native diagnostic");
        expect(localizedError(error, id).details).toBe(error.details);
        if (code !== "internal") expect(localizedError(error, id).message, `${id}/${code}`).not.toBe(catalogs[id]["error.generic"]);
      }
    }
    expect(localizedError({ code: "future_error", message: "New native error", details: null, suggestedAction: null }, "es").message)
      .toBe(catalogs.es["error.generic"]);
  });
});


describe("locale plurals", () => {
  it.each([
    ["en", 0, "updates.countdown"], ["es", 0, "updates.countdown"], ["de", 0, "updates.countdown"],
    ["fr", 0, "updates.countdownOne"], ["pt-BR", 0, "updates.countdownOne"],
    ["fr", 1, "updates.countdownOne"], ["pt-BR", 1, "updates.countdownOne"],
    ["fr", 2, "updates.countdown"], ["pt-BR", 2, "updates.countdown"], ["de", 1, "updates.countdownOne"],
  ] as const)("uses %s grammatical rules for %i", (language, count, expected) => {
    expect(pluralMessageId(language, count, { one: "updates.countdownOne", other: "updates.countdown" })).toBe(expected);
  });
});

describe("packaged permission localizations", () => {
  it("bundles a localized microphone prompt for every frontend language", () => {
    const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
    const plist = readFileSync(new URL("../src-tauri/Info.plist", import.meta.url), "utf8");
    const localizations = plist.split("<key>CFBundleLocalizations</key>")[1]!.split("</array>")[0]!;
    const bundled = [...localizations.matchAll(/<string>([^<]+)<\/string>/g)].map((match) => match[1]);
    expect(bundled.sort()).toEqual(languageOptions.map(({ id }) => id).sort());
    for (const { id } of languageOptions) {
      const resource = `locales/${id}.lproj/InfoPlist.strings`;
      expect(config.bundle.resources[resource]).toBe(`${id}.lproj/InfoPlist.strings`);
      expect(readFileSync(new URL(`../src-tauri/${resource}`, import.meta.url), "utf8"))
        .toMatch(/"NSMicrophoneUsageDescription" = "[^"]+";/);
    }
  });
});

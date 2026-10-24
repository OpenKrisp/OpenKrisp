// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider, LanguageSelector, Message, useI18n } from "./i18n";
import { languageOptions, languages, LANGUAGE_STORAGE_KEY } from "./translations";
import { UpdateNotice } from "./UpdateNotice";

const native = vi.hoisted(() => ({ isTauri: vi.fn(() => false), invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => native);
let root: Root;
let host: HTMLDivElement;
function Content() {
  const { t } = useI18n();
  return <><LanguageSelector /><h1>{t("microphone.title")}</h1>
    <Message id="microphone.select" values={{ device: <strong>{"OpenKrisp <Microphone>"}</strong> }} /></>;
}
async function mount() { await act(async () => root.render(<I18nProvider><Content /></I18nProvider>)); }
async function choose(value: string) {
  await act(async () => {
    const select = host.querySelector("select")!;
    select.value = value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
}
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  localStorage.clear();
  native.isTauri.mockReturnValue(false); native.invoke.mockReset(); native.invoke.mockResolvedValue(undefined);
  vi.spyOn(navigator, "languages", "get").mockReturnValue(["es-AR", "en"]);
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount()); host.remove();
  vi.restoreAllMocks(); vi.useRealTimers(); localStorage.clear(); document.documentElement.lang = "en";
});

describe("language preference", () => {
  it("uses regional Spanish on first launch and labels the document and controls", async () => {
    await mount();
    expect(host.querySelector("h1")?.textContent).toBe("Tu micrófono");
    expect(document.documentElement.lang).toBe("es");
    expect(host.querySelector("label")?.textContent).toBe("Idioma");
    expect(host.querySelector("select")?.value).toBe("system");
    expect(host.textContent).toContain("Selecciona OpenKrisp <Microphone> como micrófono.");
    expect(host.querySelector("strong")?.textContent).toBe("OpenKrisp <Microphone>");
    expect(host.querySelector("microphone")).toBeNull();
  });

  it("persists the explicit choice across remounts and can return to the system language", async () => {
    await mount(); await choose("en");
    expect(host.querySelector("h1")?.textContent).toBe("Your microphone");
    expect(document.documentElement.lang).toBe("en");
    expect(localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe("en");
    await act(async () => root.unmount()); root = createRoot(host); await mount();
    expect(host.querySelector("select")?.value).toBe("en");
    await choose("system");
    expect(host.querySelector("h1")?.textContent).toBe("Tu micrófono");
  });

  it("updates on system language changes while respecting an explicit choice", async () => {
    await mount();
    vi.spyOn(navigator, "languages", "get").mockReturnValue(["en-US"]);
    await act(async () => window.dispatchEvent(new Event("languagechange")));
    expect(host.querySelector("h1")?.textContent).toBe("Your microphone");
    await choose("es");
    await act(async () => window.dispatchEvent(new Event("languagechange")));
    expect(host.querySelector("h1")?.textContent).toBe("Tu micrófono");
  });

  it("ignores corrupt preferences and falls back to English for unsupported system languages", async () => {
    localStorage.setItem(LANGUAGE_STORAGE_KEY, "not-a-locale");
    vi.spyOn(navigator, "languages", "get").mockReturnValue(["ja-JP"]);
    await mount();
    expect(host.querySelector("select")?.value).toBe("system");
    expect(host.querySelector("h1")?.textContent).toBe("Your microphone");
  });

  it("keeps switching available if preference storage cannot be read or written", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("unavailable"); });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("unavailable"); });
    await mount(); await choose("en");
    expect(host.querySelector("h1")?.textContent).toBe("Your microphone");
    expect(host.querySelector('[role="status"]')?.textContent).toContain("could not be saved");
  });

  it("synchronizes native menus at startup and after language changes", async () => {
    native.isTauri.mockReturnValue(true);
    await mount();
    expect(native.invoke).toHaveBeenLastCalledWith("set_app_language", { language: "es" });
    await choose("en");
    expect(native.invoke).toHaveBeenLastCalledWith("set_app_language", { language: "en" });
  });

  it("shows a translated retry instruction if native menu updates fail", async () => {
    native.isTauri.mockReturnValue(true); native.invoke.mockRejectedValue(new Error("native menus unavailable"));
    await mount();
    expect(host.querySelector('[role="status"]')?.textContent).toContain("No se pudo actualizar el idioma del menú");
  });

  it("synchronizes preference changes made by another window", async () => {
    await mount(); localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
    await act(async () => window.dispatchEvent(new StorageEvent("storage", { key: LANGUAGE_STORAGE_KEY })));
    expect(host.querySelector("h1")?.textContent).toBe("Your microphone");
  });

  it("does not reset the update countdown when the language changes", async () => {
    vi.useFakeTimers();
    const updates = { status: { phase: "ready" as const, revision: 1, version: "1.0.0", downloadedBytes: 1, totalBytes: 1, error: null },
      installing: false, error: null, install: vi.fn(), check: vi.fn() };
    await act(async () => root.render(<I18nProvider><LanguageSelector /><UpdateNotice updates={updates} helpOpen={false} audioBusy={false} /></I18nProvider>));
    await act(async () => vi.advanceTimersByTimeAsync(20_000));
    await choose("en");
    expect(host.textContent).toContain("10 seconds");
    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    expect(updates.install).toHaveBeenCalledTimes(1);
  });
  it.each([
    ["pt-BR", "Seu microfone"], ["fr", "Votre microphone"], ["de", "Dein Mikrofon"],
  ] as const)("persists %s across remounts and synchronizes native menus", async (id, title) => {
    native.isTauri.mockReturnValue(true);
    await mount(); await choose(id);
    expect(host.querySelector("h1")?.textContent).toBe(title);
    expect(document.documentElement.lang).toBe(id);
    expect(host.querySelector(".language-display span[lang]")?.textContent).toBe(languages[id].shortName);
    expect(host.querySelector("#app-language-current")?.textContent).toContain(languages[id].name);
    expect(localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBe(id);
    expect(native.invoke).toHaveBeenLastCalledWith("set_app_language", { language: id });
    expect([...host.querySelectorAll("option")].map((option) => option.value)).toEqual(["system", ...languageOptions.map((option) => option.id)]);
    await act(async () => root.unmount()); root = createRoot(host); await mount();
    expect(host.querySelector("h1")?.textContent).toBe(title);
    expect(host.querySelector("select")?.value).toBe(id);
  });

  it.each([
    ["pt_BR", "pt-BR", "Seu microfone"], ["fr-CA", "fr", "Votre microphone"], ["de-CH", "de", "Dein Mikrofon"],
  ])("detects %s without saving an explicit preference", async (systemLocale, id, title) => {
    vi.spyOn(navigator, "languages", "get").mockReturnValue([systemLocale, "en"]);
    await mount();
    expect(document.documentElement.lang).toBe(id);
    expect(host.querySelector("h1")?.textContent).toBe(title);
    expect(host.querySelector("select")?.value).toBe("system");
    expect(localStorage.getItem(LANGUAGE_STORAGE_KEY)).toBeNull();
  });

});

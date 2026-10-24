# Desktop languages

OpenKrisp supports English (`en`), Spanish (`es`), Brazilian Portuguese (`pt-BR`),
French (`fr`), and German (`de`). The language picker in the header is available
on both the main screen and Help. It shows a globe and the effective language name;
the native menu offers the saved preference, including **System default**. Help and
Back keep accessible labels when their text is hidden at narrow widths.
**System default** uses the first supported entry in
the operating system/WebView language preference list, including regional variants
such as `es-AR`, `es-ES`, `fr-CA`, and `de-CH`. Exact locale matches take precedence
within each system preference, followed by the available translation for that language.
`pt`, `pt-BR`, and other Portuguese regional variants use the available Brazilian
Portuguese translation. This does not provide a separate European Portuguese catalog.
If none is supported, the app uses English.

An explicit language choice overrides the system language and is
saved in the WebView's local storage under `openkrisp.language`. This preference is
independent of audio settings. Switching languages updates the mounted interface,
its accessibility labels, the tray, and the macOS application menu immediately.
It neither starts nor stops audio. Returning to **System default** follows subsequent
system language changes. If storage is unavailable, the choice still applies for
the current session and a translated message explains that it was not saved.

Device names, including **OpenKrisp Microphone** and **OpenKrisp Speaker**, are kept
exactly as reported by the audio backend so they match other applications. Sanitized
support JSON retains its existing stable format and English technical labels. Native
errors show translated summaries and recovery guidance based on stable error codes;
**Technical details / Detalles técnicos** preserves the original native message and
recovery suggestion. Update failures similarly retain their original diagnostic text
in expandable technical details. macOS microphone permission text is bundled in
all five languages; macOS chooses the prompt's language using its own preferences.
Installer and standalone uninstaller interfaces are outside the desktop's language
preference.

## Adding or changing messages

- Add the English message with a stable identifier to
  `apps/desktop/src/locales/en.ts`; this defines the `MessageId` type.
- Supply every message in each supported catalog in `apps/desktop/src/locales/`.
  Each must satisfy `Record<MessageId, string>`, so missing and unexpected keys fail typechecking.
- Use `useI18n().t(id, values)` for plain text. Translate complete sentences, keeping
  interpolation parameters such as `{device}` and `{seconds}` consistent.
- Use `<Message id="…" values={{ device: <strong>{name}</strong> }} />` for sentences
  with emphasized values. This preserves translator word order and React escaping.
- Pure validation helpers accept a translator as their last argument and default to
  English outside the interface. Never store their translated text as application
  state. Store native errors and message identifiers so switching languages also
  updates existing notices.
- Use `pluralMessageId` for count-dependent messages. It selects the grammatical
  category using `Intl.PluralRules`, including French and Brazilian Portuguese zero
  forms, with an explicit `other` fallback.

## Adding another language

Add its complete catalog and register it in the `languages` object in `translations.ts`.
The registry owns canonical identifiers, full native names, compact header names,
base-language matching, and catalogs. The preference type, validation, picker, and
catalog inventory are derived from this registry. Use a regional identifier where
needed, such as `pt-BR`, and keep the full native name in the menu even when the
header uses a shorter name.

Extend the native `AppLanguage` enum, `ALL` inventory, and every label array in
`src-tauri/src/language.rs` in matching order. Add the microphone prompt in
`src-tauri/locales/<id>.lproj/InfoPlist.strings`, register the language in `Info.plist`,
and include the file in the Tauri resource map. The tests check catalog completeness,
interpolation, packaged prompts, native values, and menu switching between every pair
of supported languages. Existing catalogs are bundled locally; no translation service
or network request is involved.

## Checks

Run `npm test --prefix apps/desktop` and `npm run --prefix apps/desktop build`.
Tests cover catalog completeness and placeholder parity, regional/system language
resolution, explicit choices across remounts, unavailable storage, system changes,
native menu synchronization, setup/error/support guidance in all added languages,
and switching languages during running audio and an update countdown. The desktop Rust tests
check supported native language values, reversible menu labels, and the capability
allowlist. Run desktop Clippy with warnings denied and `cargo ci-fmt` after native
changes.

Visual checks should include the main screen and Help in every supported language
at the 1080 × 720 default and 360px minimum width, with long device names. macOS/Linux
native menu and operating system permission prompts should also be checked on each
release target. Native-speaker proofreading remains a release check for new translations.
These checks do not qualify physical audio or call quality.

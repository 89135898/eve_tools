# Desktop Internationalization Design

Date: 2026-05-26

## Purpose

Add multilingual UI support to the Tauri desktop app using `react-i18next`. The first supported languages are Simplified Chinese (`zh-CN`) and English (`en-US`).

## Scope

In scope:

- Language selector in the desktop top bar.
- Persisted language preference in `localStorage`.
- Browser-language detection with fallback to `zh-CN`.
- Translation of static UI copy in `App.tsx`.
- Translation of backend-returned stable codes such as sync status, data quality, side/action values, and reason codes.
- Locale-aware number formatting for displayed counts and daily volume.

Out of scope:

- Rust-side localization.
- Persisting language preference in SQLite.
- Translating EVE item names.
- Translating free-form error strings from failed Tauri commands.
- Additional languages beyond `zh-CN` and `en-US`.

## Architecture

Use `i18next` and `react-i18next` in the React layer only.

New frontend files:

- `apps/desktop/src/i18n/index.ts`: initializes i18next, exports supported language metadata and helpers.
- `apps/desktop/src/i18n/resources.ts`: translation resources for `zh-CN` and `en-US`.

Existing frontend files:

- `apps/desktop/src/main.tsx`: import i18n initialization before rendering.
- `apps/desktop/src/App.tsx`: use `useTranslation()` for UI strings, language selector, code mapping, and number formatting.

The Rust domain and Tauri command boundary continue returning stable codes. React translates codes at display time.

## Language Behavior

Language resolution order:

1. `localStorage["evetools.language"]`
2. `navigator.language`
3. `zh-CN`

Only `zh-CN` and `en-US` are accepted. Unsupported browser language variants fall back by prefix where possible:

- `zh`, `zh-Hans`, `zh-CN` -> `zh-CN`
- `en`, `en-US`, `en-GB` -> `en-US`
- anything else -> `zh-CN`

Changing the selector calls `i18n.changeLanguage(language)` and persists the exact supported language key.

## Translation Keys

Static UI text uses namespaced keys:

- `app.*`
- `actions.*`
- `statusCards.*`
- `lookup.*`
- `selection.*`
- `orders.*`
- `language.*`

Backend code mappings use dedicated namespaces:

- `codes.syncStatus.*`
- `codes.dataQuality.*`
- `codes.side.*`
- `codes.action.*`
- `codes.reason.*`

Missing code translations should display the original code string so new backend codes remain visible during development.

## Testing

Validation commands:

```sh
pnpm --filter @evetools/desktop typecheck
pnpm check
```

Manual verification:

- Start the app with `pnpm dev`.
- Confirm default UI appears in Chinese on unsupported or Chinese browser language.
- Switch to English and confirm static UI, sync statuses, data quality, side/action values, reason codes, and number formatting update.
- Refresh/restart and confirm the selected language persists.

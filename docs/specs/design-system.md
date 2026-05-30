# Design System & UX Spec

> **Status**: Live. Tokens are in `app/src/styles.css` (single `@theme`, light Hub + the
> floating HUD). Shared primitives (`Button`, `Card`, `Field`, `Toggle`, `Pill`, `NavItem`,
> `PageHeader`, `StatTile`, `MicHud`) live in `app/src/lib/components/ui/`. The Settings/Hub
> window is a **sidebar + content** shell (`Hub.svelte`) that routes between self-contained
> views (`lib/components/views/` + the CRUD sections). Onboarding and the mic HUD consume the
> same tokens.
> **Last updated**: 2026-05-30
> **Coverage**: Theme, Fonts, Colors, Typography, Spacing, Radius, Elevation, Components,
> Layout (sidebar), UX (principles + flow), Accessibility, Do/Don't
> **Environment**: desktop (Windows, native)
> **Source of truth**: `app/src/styles.css` (single `@theme` block) + the `ui/` primitives.
> Derived from the **Lpalo "Blush Playground"** reference
> (`styles.refero.design/style/8033959a-dffa-4da0-b700-1af46f13c51f`), adapted from a marketing
> page to a dense desktop utility.

MIA's visual identity is **"Blush Playground"** (Lpalo): a soft blush-pink canvas, bold charcoal
typography, **2px charcoal outlines for definition instead of shadows**, generously rounded /
pill-shaped controls, and a playful-but-restrained accent palette used for fills and cards —
never large text blocks. It reads warm, confident, and a little whimsical, while staying calm
and legible for a tool that mostly gets out of the way.

There are **two surfaces**:

1. **Settings / "The Hub" window** — the blush canvas with a left **sidebar** for navigation and
   a scrollable content area. This is where onboarding, dictation settings, models/engine, the
   dictionary, snippets, per-app styles, and usage stats live. See [`settings.md`](./settings.md)
   and [`onboarding.md`](./onboarding.md).
2. **Floating mic HUD** — a small always-on-top pill that appears only while dictating. It is the
   same blush language (white pill, **2px charcoal outline**, pumpkin waveform) so it stays
   on-brand, and the heavy outline keeps it legible floating over *any* application behind it. See
   [`tray-and-hud.md`](./tray-and-hud.md).

This spec is the **source of truth** for tokens, components, and UX. [`CLAUDE.md`](../../CLAUDE.md)
may summarize it; on any conflict, this file wins.

---

## Scope decisions (locked)

- **One visual language, two surfaces.** Both the Hub and the HUD use the Blush Playground tokens.
  The HUD is not a separate dark theme — it is a solid white pill with the same charcoal outline.
- **Tailwind CSS v4.** **All tokens live in a single `@theme` block** in `app/src/styles.css`.
  Components use the generated utilities — **never raw hex**.
- **Svelte 5 (runes).** Shared primitives live in `app/src/lib/components/ui/`. The UI is a **thin
  webview** — no dictation logic (see [`architecture.md`](./architecture.md)); it renders state
  pushed from Rust and calls typed `invoke()` wrappers.
- **Two type families, by role**: **Alfa Slab One** for display/headings, **Manrope** for body and
  UI. No other typefaces. Both bundled offline via `@fontsource` (no CDN — privacy-first).
- **Definition by outline, not shadow.** Lpalo uses **no shadows and no gradients**. Surfaces are
  separated by a 2px charcoal border + background color. This is a hard rule (see §6, §10).
- **Generous rounding only.** No sharp corners: cards `10px`, accent cards `28px`, all
  interactive controls (buttons, nav, fields, badges, HUD) are **pill** (`rounded-pill`).
- **Adapted scale.** The Lpalo source is a marketing page (25px body, 120px display). MIA is a
  dense settings app, so the type scale is **scaled down** (see §3) while keeping the flavor:
  Alfa Slab One headings, Manrope body, the outline/pill/no-shadow language, the accent palette.

---

## 1. Fonts

| Token | Family | Role | Weights |
|---|---|---|---|
| `font-display` | **Alfa Slab One** | Display headlines, page/section titles, big stat values, inline emphasis (hotkey chord) | 400 (only weight) |
| `font-body` | **Manrope** | Body, labels, nav, buttons, HUD label | 400 / 500 / 700 / 800 |

```css
--font-display: "Alfa Slab One", ui-serif, Georgia, "Times New Roman", serif;
--font-body: "Manrope", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont,
  "Segoe UI", Roboto, sans-serif;
```

Bundled offline via `@fontsource/alfa-slab-one` + `@fontsource/manrope` (imported in
`src/main.ts`); `body` defaults to `font-body`. There is **no `font-semibold` (600)** — Manrope
600 is not loaded; use `font-bold` (700) for emphasis and titles.

> pt-BR note: both families ship `latin` + `latin-ext` subsets, so all Brazilian Portuguese
> diacritics (ã, õ, ç, á, é, í, ó, ú, â, ê, ô) render correctly. pt-BR is a first-class language
> (see [`speech-to-text.md`](./speech-to-text.md)).

---

## 2. Color tokens

Use the Tailwind utility, never the raw hex.

### 2a. Surfaces & ink

| Token / utility suffix | Hex | Role |
|---|---|---|
| `canvas` | `#F6E0DB` | Page background (surface 0); inset list rows; hover wash on white. |
| `canvas-deep` | `#EFD2CA` | Sidebar background, scrollbar thumb, ghost-button hover. |
| `surface` | `#FFFFFF` | Cards, fields (surface 1). |
| `charcoal` | `#000000` | Primary text, **all outlines/borders**, active nav fill text. |
| `ink-soft` | `#5B4F4A` | Secondary / helper / hint text on light surfaces. |
| `hairline` | `#E3C9C1` | Subtle inner dividers where a full 2px charcoal rule is too heavy. |

### 2b. Accent palette (fills / cards / iconography — never large text)

| Token / utility suffix | Hex | Role |
|---|---|---|
| `pumpkin` | `#EF724F` | **Active nav**, primary accent fill, "recommended"/progress badges, HUD waveform, focus ring. |
| `bubblegum` | `#981082` | Secondary accent fill (pairs with `text-surface`). |
| `sky` | `#84BFFF` | Card / stat-tile fill, info badge. |
| `seafoam` | `#ACE2DF` | Soft accent fill (the "ready to dictate" hero card). |
| `lavender` | `#E69DFF` | Soft accent fill / stat tile. |
| `lemon` | `#E7DB4C` | Highlight splash / stat tile. |
| `spring` | `#6ED311` | Positive / "on" — Toggle on-state, success pill, level meter. |
| `deep-blue` | `#5196FF` | Precise accent (reserved). |

### 2c. Semantic status

The accent palette doesn't carry reliable error/ok hues, so two semantic tokens are kept:

| Token | Hex | Role |
|---|---|---|
| `success` | `#1A7F4B` | Success text + "✓ instalado" / "inserted" tick. |
| `danger` | `#C2362F` | Error text, error card outline, danger-button outline, HUD error glyph. |

> Keep the palette tight. Add a `--color-*` token before using a new shade.

---

## 3. Typography

Family by role (§1). Each size utility carries its line-height in the `@theme` block.

| Utility | Size | Line height | Family | Use |
|---|---|---|---|---|
| `text-caption` | 12px | 1.5 | body | badges, tiny labels, version |
| `text-body` | 14px | 1.6 | body | small / helper / HUD label |
| `text-body-lg` | 16px | 1.55 | body | body default, field labels, buttons |
| `text-title` | 22px | 1.2 | **display** | card / section titles |
| `text-page` | 32px | 1.08 | **display** | page header (`PageHeader`), big stat values |
| `text-hero` | 46px | 1.04 | **display** | onboarding welcome headline (largest MIA uses) |

Weights: `font-normal` 400 (body) · `font-medium` 500 · `font-bold` 700 (labels, buttons, titles,
emphasis) · `font-extrabold` 800 available. Alfa Slab One is single-weight (400) and only used via
`font-display`. Letter spacing: normal.

---

## 4. Spacing

Tailwind v4's default 4/8px scale (no custom `--spacing-*` tokens). Typical values: content area
padding `px-10 py-9`; sidebar `px-3`–`px-6`; card padding `p-6` (default) / `p-5` (accent cards);
inter-card rhythm `gap-6` (24px); field/label gap `gap-1.5`. The HUD pill uses `px-4 py-2`.

---

## 5. Radius

| Element | Value | Token / utility |
|---|---|---|
| default cards / inset list rows | 10px | `--radius-card` → `rounded-card` |
| accent cards / stat tiles | 28px | `--radius-bubble` → `rounded-bubble` |
| **buttons, nav, fields, selects, badges, HUD, toggles** | full pill | `--radius-pill` → `rounded-pill` |
| inline code chips | 6px | `rounded-md` |
| multiline textarea | 20px | `rounded-[20px]` |

No sharp corners anywhere (Lpalo).

---

## 6. Elevation — outline, not shadow

**There are no shadow tokens.** Definition comes from a **2px charcoal border** (`border-2
border-charcoal`) plus the background-color step between `canvas` → `surface` / accent. This holds
on **both** surfaces:

- **Cards / panels**: `border-2 border-charcoal` on a white or accent fill.
- **Buttons / nav / inputs**: 2px charcoal outline; hover "lift" is a 0.5px upward `translate`,
  not a shadow.
- **Mic HUD pill**: a solid white pill with the same 2px charcoal outline — the outline alone
  separates it from whatever app is behind it.
- **Focus**: a 4px **pumpkin** ring at low opacity (`focus-visible:ring-4 ring-pumpkin/45`), never
  a shadow. (See §9c.)

No gradients (Lpalo) — solid color blocks only.

---

## 7. Components

Implemented in `app/src/lib/components/ui/` as Svelte 5 components.

### Button (`Button.svelte`)
Pill, 2px outline, `font-body font-bold`, ≥40px hit target, hover `-translate-y-0.5`. Variants:
- **primary**: `bg-charcoal text-surface border-charcoal` — the unambiguous action (Download, Add,
  Test). Charcoal fill keeps it crisp and high-contrast.
- **secondary**: `bg-surface text-charcoal border-charcoal hover:bg-canvas` — outline pill.
- **ghost**: transparent until hover (`hover:border-charcoal hover:bg-canvas-deep`) — tertiary
  (Remove, Cancel, Reset).
- **danger**: `bg-surface text-danger border-danger hover:bg-danger hover:text-surface`.
- Sizes: `md` (default) / `sm` (inline list actions). `pumpkin` is **not** a button fill — it is
  reserved for active navigation and accent emphasis.

### Card (`Card.svelte`)
`border-2 border-charcoal`. `tone="surface"` (default) → `bg-surface rounded-card p-6`. Accent
tones (`pumpkin`/`bubblegum`/`sky`/`seafoam`/`lavender`/`lemon`/`spring`) → vivid fill,
`rounded-bubble p-5` (`bubblegum` flips text to `text-surface`). No shadow, ever.

### Field (`Field.svelte`)
Label (`text-body-lg font-bold text-charcoal`) above the control, optional hint (`text-body
text-ink-soft`). The control class comes from `inputClass` / `textareaClass` (`ui/inputClass.ts`):
pill outline input, `focus-visible:ring-4 ring-pumpkin/45`, `placeholder:text-ink-soft`.

### Toggle (`Toggle.svelte`)
Pill track, 2px charcoal outline, charcoal knob. Off: `bg-surface`. On: `bg-spring`. Always paired
with a text label — never color-only (§9c). Used for "launch at login", "snippets enabled",
"per-app enabled".

### Pill / Badge (`Pill.svelte`)
Pill, 2px charcoal outline, `text-caption font-bold`. Tones (fill + always a text label):
`neutral` (white), `success` (spring), `danger` (red, white text), `accent` (pumpkin),
`info` (sky). Carries the "100% local · offline" trust marker (`info`) and status (downloading,
installed, warm/cold engine).

### NavItem (`NavItem.svelte`)
Sidebar pill. Active = `border-charcoal bg-pumpkin` (the Lpalo active-nav treatment); inactive =
`border-transparent hover:border-charcoal hover:bg-surface`. Leading emoji glyph is decorative
(`aria-hidden`). Sets `aria-current="page"` when active.

### PageHeader (`PageHeader.svelte`)
Per-view header: Alfa Slab One `text-page` title + optional `text-body-lg text-ink-soft` subtitle,
plus an optional right-aligned `action` snippet (e.g. the section's enable Toggle).

### StatTile (`StatTile.svelte`)
Overview metric: big Alfa Slab One value (`text-page`) over a vivid accent fill, `rounded-bubble`,
2px charcoal outline. Tone per tile (sky / lavender / lemon / spring …).

### Mic HUD pill (`MicHud.svelte`) — signature component
A small always-on-top frameless pill: `rounded-pill border-2 border-charcoal bg-surface px-4 py-2
text-charcoal`. Layout: `[ status glyph / waveform ] [ label ]`. Driven entirely by one state value
pushed from Rust (see [`dictation.md`](./dictation.md) and [`tray-and-hud.md`](./tray-and-hud.md)).

| State | Visual | Notes |
|---|---|---|
| `idle` | hidden | No resting pill. |
| `listening` | **pumpkin waveform** bars pulsing + scaled by live RMS (`hud://level`) | "You are being heard." |
| `transcribing` | small spinner (`border-hairline border-t-pumpkin`), label "Transcrevendo…" | Whisper running. |
| `inserting` | `success` ✓ tick, label "Inserido" | brief, then fade. |
| `error` | `danger` ⚠ glyph + short label | auto-dismiss; details surface in the Hub. |

The HUD **never steals focus** (no-activate, always-on-top tool window) and is click-through where
the OS allows.

---

## 8. Layout

### 8a. Settings / Hub window — sidebar shell (`Hub.svelte`)
- **Background** `canvas`. The whole window is a flex row: a fixed-width **sidebar** + a scrollable
  content area.
- **Sidebar** (`w-[244px]`, `bg-canvas-deep`, `border-r-2 border-charcoal`): the `MIA` wordmark
  (Alfa Slab One) at top, the "100% local · offline" `info` pill, then the nav (`NavItem`s), and a
  footer with the signed-update button (or the version label). Sections:
  **Visão geral · Ditado · Modelos & Motor · Dicionário · Snippets · Por app**.
- **Content area** (`max-w-[820px]`, `px-10 py-9`): one view at a time. Each view opens with a
  `PageHeader`, then `Card`s with `gap-6` rhythm. Section titles use `font-display text-title`;
  helper text `text-ink-soft`.
- Each view is **self-contained** — it calls the typed `invoke()` wrappers itself
  (`OverviewView`, `DictationView`, `ModelsView`, and the `DictionarySection` / `SnippetsSection` /
  `PerAppSection` CRUD views). The shell owns only navigation + the update affordance.
- Window is **resizable** (min 720×520); native Windows title bar (decorations on). WebView2 host.

### 8b. Floating mic HUD (frameless, always-on-top)
- A **frameless, transparent, no-activate, always-on-top** Tauri window holding only the `MicHud`
  pill. No title bar, no chrome.
- **Positioning** (priority): near the caret when obtainable; else screen-anchored fallback
  (default bottom-center, above the taskbar), user-configurable in [`settings.md`](./settings.md).
- Appears on `listening`, disappears (fade) after `inserting`/`error`. Sized to content.

---

## 9. UX

### 9a. Design principles

1. **Latency-first.** The product is judged on time-to-text. The HUD shows `listening` *instantly*
   on hotkey press (before any model work); the warm/resident STT (ADR-004) starts transcription
   without a cold load. Never animate or block the path to inserted text.
2. **Faithful, not creative, by default.** MIA types **what you said**, cleaned up — not reworded.
   Rule-based cleanup is always-on; LLM polish/commands are descoped (Phase 2). UI defaults reflect
   this: fidelity on, magic off.
3. **Unobtrusive.** The HUD is small, present only while dictating, click-through, no sound by
   default. The app's home is the system tray ([`tray-and-hud.md`](./tray-and-hud.md)).
4. **Keyboard-first.** The whole point is a global push-to-talk hotkey ([`hotkeys.md`](./hotkeys.md)).
   The Hub is fully keyboard-navigable; every control reachable without a mouse.
5. **Permission-honest.** Be explicit about the mic permission, that **voice never leaves the
   machine**, that the first model is a one-time download, and that synthetic input **cannot reach
   elevated/UAC windows** unless MIA is elevated (ADR-005). Plain text, never buried.

### 9b. End-to-end interaction flow

```
[idle: tray icon only, HUD hidden]
   │  user presses & holds the push-to-talk hotkey (works unfocused)
   ▼
[HUD appears → listening]   ← pumpkin waveform driven by the live mic level meter
   │  user speaks (cpal 16 kHz mono; Silero VAD gates silence)
   ▼  user releases the key (push-to-talk) or toggles off
[HUD → transcribing]        ← spinner; warm whisper.cpp runs on the buffer
   ▼
[text inserted at the cursor]  ← SendInput Unicode at the focused app's caret (ADR-005);
   │                              deterministic cleanup applied before injection
   ▼
[HUD → inserting → brief ✓ → fade out]  →  [idle]
```

The user's focused application keeps focus throughout — the HUD never activates.

### 9c. Accessibility

- **Visible focus rings** on every interactive control: 4px **pumpkin** at low opacity
  (`focus-visible:ring-4 ring-pumpkin/45`). Never remove without replacing.
- **Hit targets ≥ 40px** for all Hub controls.
- **Never color-only.** Every state pairs color with text and/or icon — Toggle has on/off + a
  label, the HUD states each carry a label, status pills carry a word.
- **Contrast.** `charcoal` on `canvas`/`surface` is maximal. `ink-soft` (#5B4F4A) on `canvas`/
  `surface` clears AA for secondary text. On accent fills, body text is `charcoal` (the palette was
  chosen so charcoal-on-accent stays legible); `bubblegum` is dark enough to take `text-surface`.
- **HUD must not steal focus or trap input** — no-activate, click-through overlay.
- **Reduced motion.** Honor `prefers-reduced-motion`: the HUD waveform animation is disabled
  (`@media (prefers-reduced-motion: reduce)`), leaving static bars; button hover-lift is a tiny
  transform only.

---

## 10. Do / Don't

**Do**
- Use `font-display` (Alfa Slab One) for titles/headlines and `font-body` (Manrope) for everything
  else, on both surfaces.
- Define every surface with a **2px charcoal outline** + background step.
- Reserve `pumpkin` for active navigation, accent emphasis, and the HUD waveform/focus ring.
- Keep all interactive controls **pill-shaped**; cards `rounded-card`, accent cards `rounded-bubble`.
- Pair every state with text/icon, not color alone.
- Bundle fonts offline via `@fontsource` (no CDN — privacy-first).

**Don't**
- **No shadows. No gradients.** Definition is outline + color only.
- **No sharp corners** — everything is rounded (10 / 28 / pill).
- No `font-semibold` (600 isn't loaded) — use `font-bold`.
- No second display/body typeface beyond Alfa Slab One + Manrope.
- No accent color behind large blocks of text — accents are fills/cards/icons.
- No `pumpkin`-filled buttons (pumpkin = nav/accent); the primary button is charcoal.
- Don't let the HUD activate, steal focus, or block the app under the cursor.

---

## 11. Out of scope (V1)

- A separate dark theme for the Hub (one blush language across both surfaces).
- A heavy motion/animation system beyond the HUD waveform, hover transforms, and success/error
  ticks.
- Mobile / touch layouts — MIA is Windows-desktop-only in V1 (ADR-011).
- A custom (frameless) Hub title bar — the native Windows chrome is kept in V1.

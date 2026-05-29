# Design System & UX Spec

> **Status**: Phase 1 — tokens live in `app/src/styles.css` (`@theme`, light + HUD); the shared primitives (`Button`, `Card`, `Field`, `Toggle`, `Pill`) exist in `app/src/lib/components/ui/` and a first Settings/Hub surface consumes them. Remaining: the floating mic HUD window styling + onboarding screens.
> **Last updated**: 2026-05-29
> **Coverage**: Theme, Fonts, Colors (light + HUD), Typography, Spacing, Radius, Elevation, Components, Layout, UX (principles + flow), Accessibility, Do/Don't
> **Environment**: desktop (Windows, native)
> **Source of truth (when built)**: `app/src/styles.css` (single `@theme` block); shared
> components in `app/src/lib/components/ui/`. Derived from the Toolzy token system + the
> Calendly "Sky Blueprint on Bright Paper" reference the owner provided.

MIA's visual identity is **"Calm Focus"**: a quiet, trustworthy, latency-first dictation tool
that mostly stays out of the way. There are exactly **two surfaces**, each with its own job:

1. **Settings / "The Hub" window — light theme.** Adopts the proven Toolzy token system and the
   Calendly "Sky Blueprint on Bright Paper" palette so the owner's portfolio (Toolzy, Financo,
   MIA) stays visually cohesive: bright paper, deep indigo text, a single confident action-blue,
   soft slate-tinted shadows, generous spacing, rounded corners. This is where onboarding,
   settings, the dictionary, snippets, and stats live. See [`settings.md`](./settings.md) and
   [`onboarding.md`](./onboarding.md).
2. **Floating mic HUD — dark, translucent.** A small always-on-top pill/overlay that appears
   only while dictating. Because it floats *over the user's other apps* it must be dark,
   translucent, low-distraction, and click-through where possible. It is the only place where a
   dark surface is allowed in V1. See [`tray-and-hud.md`](./tray-and-hud.md).

This spec is the **source of truth** for tokens, components, and UX. [`CLAUDE.md`](../../CLAUDE.md)
may summarize it; on any conflict, this file wins.

---

## Scope decisions (locked)

- **Two surfaces, two themes.** Settings/Hub = **light** (Calendly palette). Mic HUD = **dark,
  translucent**. A full dark theme for the settings window is **out of scope for V1** (see §13).
- **Tailwind CSS v4** is the implementation. **All tokens live in a single `@theme` block** in
  `app/src/styles.css`. Components use the generated utilities — **never raw hex**.
- **Svelte 5 (runes)** for the UI. Shared primitives live in `app/src/lib/components/ui/`. The
  UI is a **thin webview** — it holds no dictation logic (see [`architecture.md`](./architecture.md));
  it renders state pushed from Rust and calls typed `invoke()` wrappers.
- **One type family**: Montserrat (see §1). No decorative typefaces.
- **8px base unit** for spacing; Tailwind's stock scale (no custom `--spacing-*` tokens).
- **One action color** discipline carries to *both* surfaces: light uses `action-blue` for the
  primary path; the HUD uses the same `action-blue` as its "listening" accent. No second CTA color.

---

## 1. Font decision

The Calendly reference uses **Gilroy**, a **commercial/proprietary** font — it cannot be bundled
in an open-source MIT project. The reference's own documented substitute is **Montserrat** (SIL
Open Font License, free, geometric, very close match).

**Decision:** ship **Montserrat** as the actually-loaded family (bundled offline via
`@fontsource/montserrat` — consistent with MIA's no-network, privacy-first stance; no Google
Fonts CDN call), and keep the token name `--font-gilroy` so component code and the shared
portfolio vocabulary stay aligned with Toolzy.

```css
--font-gilroy: "Montserrat", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont,
  "Segoe UI", Roboto, sans-serif;
```

If a licensed Gilroy is ever purchased, swap the `--font-gilroy` value — no component changes
needed. Weights used: **400 / 500 / 600 / 700**.

> pt-BR note: Montserrat covers Latin-1 + Latin Extended-A, so all Brazilian Portuguese
> diacritics (ã, õ, ç, á, é, í, ó, ú, â, ê, ô) render correctly. pt-BR is a first-class
> language (see [`speech-to-text.md`](./speech-to-text.md)).

---

## 2. Color tokens

### 2a. Settings / Hub — light theme (Calendly "Sky Blueprint")

Use the Tailwind utility, never the raw hex.

| Token / utility suffix | Hex | Role |
|---|---|---|
| `midnight-indigo` | `#0B3558` | Primary text, headings, inactive nav. The branded "almost-black". |
| `action-blue` | `#006BFF` | Primary CTA, active nav/tab, key interactive accents. **The one action color.** |
| `slate-blue` | `#476788` | Secondary text, supporting info, icon fills. |
| `steel-gray` | `#A6BBD1` | Tertiary text, disabled states, fine borders. |
| `platinum-tint` | `#D4E0ED` | Inactive field borders, subtle dividers. |
| `cloud-mist` | `#F8F9FB` | Off-white section / window backgrounds. |
| `snow-white` | `#FFFFFF` | Card surfaces, elevated panels. |
| `success` | `#1A7F4B` | Success feedback (model downloaded, text inserted). Semantic status token. |
| `danger` | `#C2362F` | Failure feedback (mic blocked, model error, injection failed). Semantic status token. |

> The wider Calendly palette (glacier-blue, pale-gray, royal-amethyst, etc.) is **not tokenized**
> in MIA's `@theme` — add a `--color-*` token before using a new shade. Keep the palette tight.

### 2b. Mic HUD — dark, translucent theme

The HUD floats over arbitrary applications, so its background must read as a distinct, dark,
translucent object regardless of what is behind it. These tokens are **HUD-only** (prefixed
`hud-` in the `@theme` block) and must not be used in the light Settings window.

| Token / utility suffix | Value | Role |
|---|---|---|
| `hud-bg` | `rgba(11, 53, 88, 0.92)` | Near-black translucent pill background (dark slate / midnight-indigo @ 92%). Pairs with a `backdrop-blur`. |
| `hud-bg-solid` | `#0B3558` | Opaque fallback when the OS/WebView2 can't composite translucency. |
| `hud-text` | `#FFFFFF` (`snow-white`) | HUD label text ("Listening…", "Transcribing…"). |
| `hud-text-dim` | `rgba(255,255,255,0.64)` | Secondary HUD text (elapsed time, language tag). |
| `hud-accent` | `#006BFF` (`action-blue`) | **The "listening" accent**: the pulse ring + active waveform bars. Same one action color. |
| `hud-wave` | `rgba(0,107,255,0.85)` | Live level-meter / waveform bar fill. |
| `hud-success` | `#1A7F4B` (`success`) | Brief "inserted" check tick. |
| `hud-danger` | `#C2362F` (`danger`) | Error state (mic lost, transcription failed). |
| `hud-border` | `rgba(255,255,255,0.08)` | Hairline ring around the pill for definition over light backgrounds. |

Rule: the HUD keeps the **one-action-color discipline** — `action-blue` (`hud-accent`) is the
*only* chromatic accent; everything else is white / dim-white on dark slate, plus the semantic
success/danger ticks.

---

## 3. Typography

Family: `font-gilroy` (Montserrat). MIA needs only the small-to-mid end of Toolzy's scale (it's
a utility app, not a marketing site). Each utility carries its line-height in the `@theme` block.

| Utility | Size | Line height | Use |
|---|---|---|---|
| `text-body` | 14px | 1.71 | small / caption / HUD secondary label |
| `text-body-lg` | 16px | 1.6 | body default (settings copy) |
| `text-subheading` | 18px | 1.6 | lead paragraph, section intro |
| `text-heading` | 24px | 1.4 | card titles, tab H3 |
| `text-heading-lg` | 28px | 1.2 | window / onboarding H2 |
| `text-display-sm` | 38px | 1.21 | onboarding hero / welcome title (largest MIA uses) |

> MIA does **not** define `text-display` / `text-display-lg` (no landing-page hero in the app).
> The HUD label ("Listening…") uses `text-body` / `text-body-lg`, weight 500.

Weights: `font-normal` 400 (body) · `font-medium` 500 (nav, HUD label) · `font-semibold` 600
(titles, buttons) · `font-bold` 700 (onboarding headline). Letter spacing: normal.

---

## 4. Spacing

Base unit **8px**. Tailwind v4's default scale is already a 4/8px grid, so MIA uses the stock
utilities — there are **no custom `--spacing-*` tokens** (note Tailwind's `p-24` = 96px, not
24px). Typical values: window/section padding `px-6` (24px), tab content rhythm `py-8`/`py-12`,
card and field gaps `gap-4`/`gap-6`. The HUD pill uses tight padding `px-3 py-2` (12px/8px).
Stay on the scale to keep the 8px feel.

---

## 5. Radius

| Element | Value | Utility |
|---|---|---|
| small (chips, inline tags) | 4px | `rounded-md` |
| **buttons** | **8px** | `rounded-lg` |
| medium (fields, inputs) | 12px | `rounded-xl` |
| **cards / panels** | **16px** | `rounded-2xl` |
| **mic HUD pill** / badges / toggles | full | `rounded-full` |

---

## 6. Elevation (shadows)

### 6a. Light surfaces

Two soft, slate-tinted (`rgba(71,103,136,…)`) shadows, identical to Toolzy — `--shadow-sm` and
`--shadow-sm-2`. Map by intent:

| Intent | Token / utility |
|---|---|
| Resting / elevated card or panel | `shadow-sm-2` (deep triple-layer — featured surfaces) |
| Hover / interactive lift | `shadow-sm` |
| Button / field focus | a `focus-visible` ring (`action-blue`, 2px, offset), **not** a shadow |

Don't put heavy shadows on non-interactive, non-emphasized elements.

### 6b. HUD (dark, translucent)

The HUD reads as a floating glass object, not a paper card:

```css
/* applied to the HUD pill */
backdrop-filter: blur(16px);
box-shadow: rgba(0, 0, 0, 0.35) 0px 8px 24px 0px;  /* --shadow-hud: subtle, dark, diffuse */
```

Plus the `hud-border` hairline ring (`rgba(255,255,255,0.08)`) so the pill stays legible over a
bright window behind it. Keep HUD elevation **subtle** — it should feel light and unobtrusive,
never a heavy modal.

---

## 7. Components

Class recipes (Tailwind v4 utilities). Implemented in `app/src/lib/components/ui/` as Svelte 5
components (`Button.svelte`, `Card.svelte`, `Field.svelte`, `Toggle.svelte`, `Pill.svelte`,
`MicHud.svelte`, `ModelDownloadGate.svelte`, `HotkeyRecorder.svelte`).

### Button
- **Primary CTA**: `bg-action-blue text-snow-white rounded-lg font-semibold px-4 py-1.5`
  (lg size: `px-6 py-3 text-body-lg`). Hover: `brightness-105`.
  Focus: `focus-visible:ring-2 ring-action-blue ring-offset-2` (shared `focusRing`).
- **Ghost**: transparent, `text-midnight-indigo font-semibold rounded-lg`, optional
  `border border-platinum-tint`. Secondary/destructive-cancel actions.
- **Danger (text)**: `text-danger font-semibold` ghost for destructive confirmations
  (e.g. "Delete model"). Never the primary blue.

### Card / Panel
`bg-snow-white rounded-2xl shadow-sm-2 p-6` (24px). Interactive variant adds
`transition-shadow hover:shadow-sm`. No border by default. Used for each Hub tab section and
onboarding step.

### Field (text input)
`bg-snow-white border border-platinum-tint rounded-xl px-3 py-2 text-body-lg`.
Focus: `border-action-blue ring-2 ring-action-blue/20`. Disabled: `text-steel-gray`,
`border-platinum-tint`. Label `text-body font-medium text-slate-blue` above the control.

### Toggle (switch)
`rounded-full` track. Off: `bg-steel-gray`. On: `bg-action-blue`. White knob, slate-tinted
shadow. Focus ring as above. Always paired with a text label — never color-only (see §12). Used
heavily in [`settings.md`](./settings.md) (e.g. "Polish on insert", "Play sound on start").

### Pill / Badge
- **Native badge**: `bg-cloud-mist text-slate-blue rounded-full text-body font-semibold px-2 py-1`
  — the "100% local · offline" trust marker.
- **Status pill**: `success` / `danger` / `slate-blue` fill at low opacity with matching text,
  `rounded-full`. Always carries a text label, not just a color.

### Mic HUD pill (signature component)
A small always-on-top frameless pill (`MicHud.svelte`), `rounded-full`,
`bg-[hud-bg] backdrop-blur-[16px] text-hud-text px-3 py-2`, `shadow-hud`, `hud-border` ring.
Layout: `[ status glyph ] [ live level meter ] [ label ]`. It is driven entirely by a single
state value pushed from Rust (see [`dictation.md`](./dictation.md) and
[`tray-and-hud.md`](./tray-and-hud.md)). Visual states:

| State | Visual | Notes |
|---|---|---|
| `idle` | hidden | HUD is not shown at all (no resting pill). |
| `listening` | `hud-accent` **pulsing ring** + **live waveform** bars animated from the real mic level meter (`hud-wave`) | The "you are being heard" signal. Bars react to RMS amplitude streamed from cpal. |
| `transcribing` | small `hud-accent` **spinner**, waveform frozen/dimmed, label "Transcribing…" | Whisper is running on the captured buffer. |
| `inserting` | brief **check tick** in `hud-success`, label "Inserted" | ~400ms, then fade. |
| `error` | `hud-danger` glyph + short label (e.g. "Mic blocked", "No speech") | Auto-dismiss after a few seconds; details surface in the Hub. |

The HUD **never steals focus** (created as a no-activate, always-on-top tool window) so the
user's target app keeps the caret. It is click-through where the OS allows, so it never blocks
the app underneath.

### Model-download gate
`ModelDownloadGate.svelte` — a `Card` shown when the required Whisper model isn't present yet
(first run / engine switch). Contents: model name + size, a **primary `action-blue` "Download"
button**, an indeterminate-then-determinate progress bar (`action-blue` fill on `cloud-mist`
track; progress streamed over a Tauri Channel — reuses Toolzy's pattern, see
[`speech-to-text.md`](./speech-to-text.md) and [`REUSE-FROM-TOOLZY.md`](../REUSE-FROM-TOOLZY.md)),
and a "downloads once, then fully offline" reassurance line. Cancelable. Maps to the on-demand
"download gate" UX.

### Hotkey recorder
`HotkeyRecorder.svelte` — a `Field`-styled control that, when focused/clicked, captures the next
key chord and renders it as `rounded-md` key caps (e.g. `Ctrl` + `Space`). Shows a `danger`
inline message on a reserved/conflicting combo. Backs the push-to-talk binding in
[`hotkeys.md`](./hotkeys.md) and [`onboarding.md`](./onboarding.md).

---

## 8. Layout

### 8a. Settings / Hub window (light)
- **Background** `cloud-mist`; content panels are `snow-white` `Card`s.
- **Structure**: a left **sidebar (or top tab bar) + content area**. Sidebar holds the Hub
  sections — Dictation, Models/Engine, Dictionary, Snippets, AI (Phase 2), Stats, About/Update.
  Active item = `text-action-blue` (`font-medium`); inactive = `text-midnight-indigo`.
- **Header strip**: window title / logo + a `native` "100% local" badge, and a right-aligned
  version / "Update to vX" button (signed auto-update — see [`architecture.md`](./architecture.md)
  ADR-009).
- **Content**: one `Card` per logical group; section title `text-heading`, helper text
  `text-slate-blue`. Comfortable vertical rhythm (~32–40px between groups).
- Window is **resizable**, sensible min size; WebView2 host (ADR-002).

### 8b. Floating mic HUD (dark, frameless, always-on-top)
- A **frameless, transparent, no-activate, always-on-top** Tauri window holding only the
  `MicHud` pill. No title bar, no chrome.
- **Positioning** (in priority order):
  1. **Near the caret** — anchored just below/above the current text insertion point when its
     location is obtainable, so feedback sits where the user is typing.
  2. **Screen-anchored fallback** — a fixed corner/edge (default: bottom-center, just above the
     taskbar) when the caret position is unknown. User-configurable in
     [`settings.md`](./settings.md).
- Appears on `listening`, disappears (fade) after `inserting`/`error`. Sized to content
  (compact pill); does not grow into a panel.

---

## 9. UX

### 9a. Design principles

1. **Latency-first.** The product is judged on time-to-text. The HUD must show `listening`
   *instantly* on hotkey press (before any model work), and the warm/resident STT (ADR-004)
   exists precisely so transcription starts without a cold model load. Never animate or block
   the path to inserted text.
2. **Faithful, not creative, by default.** MIA types **what you said**, cleaned up — not
   reworded. Deterministic rule-based cleanup is always-on (Phase 1); the LLM "Polish" and
   Command Mode are **opt-in** (Phase 2, [`ai-commands.md`](./ai-commands.md)). The UI's
   defaults reflect this: fidelity on, magic off until chosen.
3. **Unobtrusive / low-distraction.** The HUD is small, dark, translucent, click-through, and
   present only while dictating. No persistent overlay, no bouncing, no sound by default. The
   app's real home is the system tray ([`tray-and-hud.md`](./tray-and-hud.md)).
4. **Keyboard-first.** The whole point is a global push-to-talk hotkey ([`hotkeys.md`](./hotkeys.md)).
   The Hub is fully keyboard-navigable; every control is reachable and operable without a mouse.
5. **Permission-honest.** Be explicit and truthful about what MIA needs and what it can't do:
   the mic permission, that **voice never leaves the machine**, that the first model is a
   one-time download, and that synthetic input **cannot reach elevated/UAC windows** unless MIA
   itself is elevated (ADR-005). Surface these as plain text, never buried.

### 9b. End-to-end interaction flow

```
[idle: tray icon only, HUD hidden]
   │  user presses & holds the push-to-talk hotkey (works unfocused)
   ▼
[HUD appears → listening]   ← pulsing action-blue ring + live waveform from the mic level meter
   │  user speaks (cpal captures 16 kHz mono; Silero VAD gates silence)
   ▼
   │  user releases the key (push-to-talk) or toggles off (toggle mode)
   ▼
[HUD → transcribing]        ← spinner; warm whisper.cpp runs on the buffer
   │
   ▼
[text inserted at the cursor]  ← SendInput Unicode at the focused app's caret (ADR-005);
   │                              deterministic cleanup applied before injection
   ▼
[HUD → inserting → brief success tick → fade out]
   │
   ▼
[idle]
```

The user's focused application keeps focus throughout — the HUD never activates. See
[`dictation.md`](./dictation.md) for the full orchestration and [`text-injection.md`](./text-injection.md)
for the injection/clipboard-fallback detail.

### 9c. Accessibility

- **Visible focus rings** on every interactive control: `action-blue`, 2px, offset
  (`focus-visible:ring-2 ring-action-blue ring-offset-2`). Never remove the outline without
  replacing it.
- **Hit targets ≥ 40px** for all clickable controls in the Hub.
- **Never color-only.** Every state pairs color with text and/or an icon — Toggle has on/off
  text, the HUD `listening`/`transcribing`/`error` states each carry a label, status pills carry
  a word. (Critical for the HUD's success/danger ticks.)
- **Contrast.** Light surface: `midnight-indigo` / `text-black` body on `snow-white`/`cloud-mist`
  is high-contrast; `slate-blue` is fine for secondary text ≥16px; `steel-gray` is disabled-only.
  HUD: `snow-white` on `hud-bg` (dark slate @92%) clears AA; `hud-text-dim` is for non-essential
  secondary labels only.
- **HUD must not steal focus or trap input** — it is a no-activate, click-through overlay, so it
  never interferes with screen readers or keyboard focus in the user's active application.
- **Reduced motion.** Honor `prefers-reduced-motion`: replace the waveform/pulse animation with a
  static `action-blue` "listening" dot and a non-spinning transcribing indicator.

---

## 10. Do / Don't

**Do**
- Use `font-gilroy` (Montserrat) for all text, on both surfaces.
- Reserve `action-blue` for the single action path (light) and the single "listening" accent (HUD).
- Apply `rounded-2xl` + `shadow-sm-2` to prominent light cards; `rounded-full` + `backdrop-blur`
  to the HUD pill.
- Keep the HUD small, dark, translucent, click-through, and visible only while dictating.
- Pair every state with text/icon, not color alone.
- Bundle Montserrat offline via `@fontsource` (no CDN — privacy-first).

**Don't**
- No second accent / CTA color on either surface.
- No light/paper styling inside the HUD; no dark styling inside the Settings window.
- No heavy shadows or large motion — this is "Calm Focus", not a flashy overlay.
- No font families beyond Montserrat.
- Don't break the 8px spacing grid.
- Never pure black `#000` — use `text-black` (`#0A0A0A`) or `midnight-indigo`.
- Don't let the HUD activate, steal focus, or block the app under the cursor.

---

## 11. Out of scope (V1)

- **Full dark theme for the Settings/Hub window.** Only the **mic HUD** is dark in V1; the Hub
  stays light. Tokens are structured (HUD tokens isolated under `hud-*`) so a future Hub dark
  theme could be added without disturbing the HUD.
- **A heavy motion/animation system** beyond the HUD's listening pulse/waveform, simple hover
  transitions, and the success/error ticks.
- **Mobile / responsive-for-touch layouts** — MIA is Windows-desktop-only in V1 (ADR-011).
- Licensed Gilroy (using the Montserrat substitute — see §1).

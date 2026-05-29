# MIA — Feature Map (Wispr Flow parity)

> **Status**: Phase 1 (Core Dictation MVP) — **core loop validated on Windows** (PTT → capture → server-side Silero-VAD-gated warm STT → cleanup → inject, pt-BR + English, with the floating HUD); Phase 0 docs complete. **Phase 2 (AI Command Mode / Polish) is descoped** by product decision — MIA stays a faithful dictation tool. See [ROADMAP.md](ROADMAP.md).
> **Last updated**: 2026-05-29
> **Environment**: desktop (Windows, native)
> **Coverage**: inventory of [Wispr Flow](https://wisprflow.ai) features mapped to MIA's local-first plan.

This document inventories the features of **Wispr Flow** (a cloud-based, account-based
voice-to-text dictation product) and maps each one to MIA's plan. MIA is the **free, open-source,
privacy-first, local** answer to Wispr Flow: same "press a hotkey, speak, polished text appears
at the cursor in any app" core, but **everything runs on the user's machine** — no cloud, no
account, no server, voice never leaves the device.

For each feature: a short description, MIA's equivalent (or "local adaptation" / "out of scope"),
the MIA spec that owns it, and the target roadmap phase. See [ROADMAP.md](ROADMAP.md) for phase
definitions and [specs/architecture.md](specs/architecture.md) for the ADRs referenced below.

**Legend** — ✅ Done · 🚧 In progress · ⬜ Planned · 💡 Backlog · ⛔ Out of scope (deliberate)

---

## Core Dictation

| Wispr Flow feature | Description | MIA equivalent | MIA spec | Phase |
|---|---|---|---|---|
| **System-wide dictation** | Global hotkey → polished text inserted at the cursor in whatever app is focused. | **Same, fully local.** Global push-to-talk hotkey → cpal mic capture → Silero VAD endpoint → warm whisper.cpp → deterministic cleanup → SendInput injection at the cursor. The product's reason to exist. (ADR-001, ADR-005) | [dictation.md](specs/dictation.md) · [hotkeys.md](specs/hotkeys.md) · [text-injection.md](specs/text-injection.md) | **1** ✅ |
| **Real-time low latency** | Wispr targets ~<700 ms p99 in its cloud. | **Local adaptation.** No network round-trip; latency is local CPU (or NVIDIA CUDA) inference. The key enabler is a **warm/resident** STT (MVP default = a warm `whisper-server` sidecar loaded once, cmake-free; whisper-rs in-process is the later optimization behind the same seam) — NOT a cold whisper-cli spawn per utterance. (ADR-004) | [speech-to-text.md](specs/speech-to-text.md) · [dictation.md](specs/dictation.md) | **1** ✅ (warm-server default; whisper-rs swap is the open optimization) |
| **Whisper Mode** (quiet/whispered speech) | Recognizes very quiet / whispered speech for use in shared/quiet spaces. | **Local adaptation, deferred.** VAD sensitivity + gain tuning for low-amplitude speech; revisit once core dictation is solid. | [audio-capture.md](specs/audio-capture.md) · [speech-to-text.md](specs/speech-to-text.md) | **5** 💡 |

---

## AI Formatting

| Wispr Flow feature | Description | MIA equivalent | MIA spec | Phase |
|---|---|---|---|---|
| **AI auto-edits** | Removes fillers (um/uh), adds punctuation, capitalization, spacing; formats lists; casing based on surrounding text. | **Local adaptation — two tiers.** **Phase 1 = deterministic, always-on**, pure-Rust cleanup: filler-word stoplist (um/uh/é/tipo/né…), spoken-punctuation substitution ("nova linha", "ponto", "vírgula"), stutter/repeat collapse, whitespace normalization, sentence-case fixer. **Phase 2 = optional** local-LLM "Polish" for richer reformatting (lists, casing). Fidelity-safe by default — _faithful, not creative_. (ADR-008) | [text-cleanup.md](specs/text-cleanup.md) · [ai-commands.md](specs/ai-commands.md) | **1** ✅ (deterministic) / **2** ⛔ Descoped (LLM Polish — depends on dropped Phase 2 local LLM) |
| **Self-correction / backtracking** | "2… actually 3" resolves to the intended value. | **Local adaptation.** Spoken-correction handling ("actually…", "I mean…") via deterministic rules where reliable, otherwise the optional Phase-2 LLM. | [text-cleanup.md](specs/text-cleanup.md) · [ai-commands.md](specs/ai-commands.md) | **1** ⬜ (simple) / **2** ⬜ (LLM) |

---

## Voice Commands / Editing

| Wispr Flow feature | Description | MIA equivalent | MIA spec | Phase |
|---|---|---|---|---|
| **Command Mode** | Transform last/selected text: "make concise", "more formal", "bulleted list", "summarize", "translate". | **Local adaptation.** Local LLM via **llama.cpp** (Qwen2.5-3B-Instruct / Llama-3.2-3B-Instruct, Q4_K_M) with **GBNF/JSON-schema constrained decoding** for reliable command parsing. Gated behind a cheap intent check so average latency stays near Phase 1. (ADR-008) | [ai-commands.md](specs/ai-commands.md) | **2** ⛔ Descoped (dropped Phase 2 local LLM) |
| **AI assistant prompt passthrough** | Speak a free-form prompt; the assistant answers/acts. | **Local adaptation.** Same local-LLM path as Command Mode (one of the routed intents). | [ai-commands.md](specs/ai-commands.md) | **2** ⛔ Descoped (dropped Phase 2 local LLM) |
| **"Hey Flow" wake word** | Hands-free activation by voice keyword. | **Local adaptation, deferred.** "Hey MIA" wake word. PTT hotkey is the v1 activation model; wake word is backlog. | [hotkeys.md](specs/hotkeys.md) | **5** 💡 |

---

## Customization

| Wispr Flow feature | Description | MIA equivalent | MIA spec | Phase |
|---|---|---|---|---|
| **Personal / custom dictionary** | Auto-learned + manual; names, jargon, acronyms. | **Same, local.** Personal vocabulary / word-replacement list applied during cleanup. Manual entry for v1; auto-learning later. | [custom-dictionary.md](specs/custom-dictionary.md) | **3** ✅ |
| **Snippets / text expansion** | Voice-triggered expansion of saved snippets. | **Same, local.** Voice-triggered text expansion. | [snippets.md](specs/snippets.md) | **3** ✅ |
| **Cross-device sync** (Pro) | Syncs settings/dictionary across devices via the cloud. | **⛔ Out of scope (deliberate).** MIA is local-first with no account/cloud (ADR-001). Settings live in app-data on the one machine; users can back up/sync the config file themselves. | — | ⛔ |

---

## Tone / Context Awareness

| Wispr Flow feature | Description | MIA equivalent | MIA spec | Phase |
|---|---|---|---|---|
| **Writing Styles** | Formal/casual presets, often per app category. | **Deterministic (no LLM).** Per-app writing styles/context keyed to the focused app's executable: pin a language, force a clipboard-vs-SendInput backend, a trailing period, or spoken-punctuation on/off. LLM-driven *tone* rewriting stays descoped. | [per-app-context.md](specs/per-app-context.md) · [settings.md](specs/settings.md) | **3** ✅ |
| **Context Awareness** | Reads text near the cursor + active app/URL via OS Accessibility / UI Automation. | **Local adaptation, later phase.** Uses **Windows UI Automation** to read nearby text / active app — strictly on-device, with read limits (no network egress, ADR-001). Heavier integration, deferred. | [ai-commands.md](specs/ai-commands.md) · [settings.md](specs/settings.md) | **3** ⛔ Descoped (depends on dropped Phase 2 local LLM) |
| **Context-aware tone matching** | Matches tone to surrounding text. | **Local adaptation.** Local-LLM tone matching built on the Windows UI Automation context above. | [ai-commands.md](specs/ai-commands.md) | **3** ⛔ Descoped (depends on dropped Phase 2 local LLM) |
| **App-specific smart handling** | Ignore placeholders; code-editor file-name memory; per-app behaviors. | **Local adaptation.** Per-app handling layered onto context awareness. | [ai-commands.md](specs/ai-commands.md) · [settings.md](specs/settings.md) | **3** ⛔ Descoped (depends on dropped Phase 2 local LLM) |

---

## Multilingual

| Wispr Flow feature | Description | MIA equivalent | MIA spec | Phase |
|---|---|---|---|---|
| **100+ languages, auto-detect, mid-sentence switching** | Broad language coverage with automatic detection and code-switching. | **Same, local.** Whisper covers ~99 languages with auto-detect. **pt-BR (Brazilian Portuguese) is first-class** — the reason MIA picked Whisper over NVIDIA Parakeet/Canary, which are trained on European Portuguese (ADR-003). English also first-class. Mid-utterance switching follows Whisper's behavior. | [speech-to-text.md](specs/speech-to-text.md) | **1** ⬜ |

---

## Platform

| Wispr Flow feature | Description | MIA equivalent | MIA spec | Phase |
|---|---|---|---|---|
| **The Hub dashboard** | Stats: word counts, wpm, streaks. | **Same, local.** Settings window / "The Hub" dashboard with local-only usage stats. No telemetry leaves the device (ADR-001). | [settings.md](specs/settings.md) | **4** ✅ |
| **iOS / Android apps (mobile)** | Native mobile dictation apps. | **⛔ Out of scope (deliberate).** MIA is **Windows-only for v1** (ADR-011). macOS/Linux are backlog; mobile is not a goal. | — | ⛔ |

---

## Privacy / Security

| Wispr Flow feature | Description | MIA equivalent | MIA spec | Phase |
|---|---|---|---|---|
| **Privacy / Zero-Data-Retention mode** | Opt-in mode where cloud does not retain audio/transcripts. | **Default, not a mode.** Because MIA is fully local, **zero data retention by anyone is the baseline** — voice never leaves the machine; there is no server to retain anything (ADR-001). This is MIA's headline difference from Wispr Flow. | [architecture.md](specs/architecture.md) · [speech-to-text.md](specs/speech-to-text.md) | **1** ✅ (inherent) |
| **SOC2 / ISO / HIPAA / SSO compliance** | Enterprise compliance certifications. | **⛔ Out of scope (deliberate).** These certify cloud/data-handling practices; with no cloud and no server, they don't apply. Privacy posture is "it never leaves your machine," which the MIT source makes auditable. | — | ⛔ |
| **Context read limits** | Caps on how much surrounding text is read for context. | **Local adaptation.** When Windows UI Automation context lands, reads are capped and on-device only. | [ai-commands.md](specs/ai-commands.md) · [settings.md](specs/settings.md) | **3** ⬜ |

---

## Team / Enterprise

| Wispr Flow feature | Description | MIA equivalent | MIA spec | Phase |
|---|---|---|---|---|
| **Shared team dictionary / snippets** | Org-wide shared vocabulary and snippets. | **⛔ Out of scope (deliberate).** No accounts/cloud (ADR-001). Dictionary/snippets are per-machine; users may share config files manually. | — | ⛔ |
| **Admin console** | Centralized team administration. | **⛔ Out of scope (deliberate).** No multi-user/cloud backend. | — | ⛔ |
| **SSO / SAML** | Enterprise single sign-on. | **⛔ Out of scope (deliberate).** MIA has no login at all (ADR-001). | — | ⛔ |

---

## MIA's deliberate differences from Wispr Flow

MIA is not trying to be a free clone — these divergences are intentional and define the product:

- **Offline vs cloud.** Wispr Flow runs STT and formatting in the cloud. **MIA runs everything
  on-device** (whisper.cpp + optional local llama.cpp). Voice never leaves the machine; there is
  no server. (ADR-001)
- **No account.** No sign-up, no login, no SSO — install and dictate. By consequence, anything that
  requires identity or a backend (cross-device sync, team dictionaries, admin, mobile, cloud
  compliance certs) is **out of scope by design**, not a missing feature.
- **Privacy by default, not by toggle.** Wispr's "Zero-Data-Retention" is an opt-in mode; for MIA,
  zero retention by third parties is the inherent baseline.
- **MIT, fully open source.** The entire app is MIT-licensed with permissive deps only (never bundle
  AGPL). The privacy claim is auditable from source. (ADR-010)
- **Windows-only v1.** A deliberate focus choice — simplest text injection (Windows SendInput) and
  the owner's platform. macOS (Accessibility/TCC) and Linux (Wayland injection) are deferred. (ADR-011)
- **Faithful, not creative, by default.** The always-on path is **deterministic** cleanup (Phase 1);
  AI reformatting/commands are **opt-in** (Phase 2) and gated behind a cheap intent check so MIA
  defaults to transcribing what you actually said, not rewriting it. (ADR-008)

---

## Related docs

- [ROADMAP.md](ROADMAP.md) — phase definitions and status.
- [specs/architecture.md](specs/architecture.md) — ADRs referenced above.
- [specs/dictation.md](specs/dictation.md) — core orchestration.
- [specs/speech-to-text.md](specs/speech-to-text.md) — Whisper engine, models, GPU, VAD, warm model.
- [REUSE-FROM-TOOLZY.md](REUSE-FROM-TOOLZY.md) — what MIA lifts from Toolzy.

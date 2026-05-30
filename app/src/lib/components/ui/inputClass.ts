/** Shared Tailwind classes for Hub text inputs/selects/textareas — a single source so
 *  the design-system field styling can't drift across the CRUD sections ("Blush
 *  Playground", design-system.md). Pill outline for single-line controls; the
 *  multiline textarea keeps a softer rounded-[20px] so tall content reads well. */
export const inputClass =
  "w-full rounded-pill border-2 border-charcoal bg-surface px-4 py-2 text-body-lg text-charcoal " +
  "min-h-[42px] outline-none placeholder:text-ink-soft focus-visible:ring-4 focus-visible:ring-pumpkin/45";

export const textareaClass =
  "w-full rounded-[20px] border-2 border-charcoal bg-surface px-4 py-3 text-body-lg text-charcoal " +
  "outline-none placeholder:text-ink-soft focus-visible:ring-4 focus-visible:ring-pumpkin/45";

/** Selects reuse the input pill but swap the native dropdown arrow for a charcoal
 *  chevron (`.mia-select` in styles.css) positioned to match the field padding. */
export const selectClass = `${inputClass} mia-select`;

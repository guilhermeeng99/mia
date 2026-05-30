/** Shared Tailwind class for Hub text inputs/selects — a single source so the design-system
 *  field styling can't drift across the CRUD sections (Calm Focus, design-system.md). The
 *  multiline textarea omits `min-h-[40px]`, so it keeps its own class. */
export const inputClass =
  "rounded-xl border border-platinum-tint bg-snow-white px-3 py-2 text-body-lg text-midnight-indigo min-h-[40px]";

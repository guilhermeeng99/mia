<script lang="ts">
  // Live mic-level meter: a pill track with a spring-fill bar driven by a 0–1 RMS level.
  // The ×600 amplification (speech RMS is small) lives here so the two call sites — the
  // DictationView mic test and the Onboarding mic step — can't drift apart.
  interface Props {
    level: number;
    class?: string;
  }
  let { level, class: cls = "h-3 w-40" }: Props = $props();
</script>

<div
  class="overflow-hidden rounded-pill border-2 border-charcoal bg-surface {cls}"
  aria-hidden="true"
>
  <div
    class="h-full bg-spring transition-[width] duration-75"
    style="width: {Math.min(100, level * 600)}%"
  ></div>
</div>

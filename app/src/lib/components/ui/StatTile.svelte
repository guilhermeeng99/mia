<script lang="ts">
  // A single overview metric rendered as a Lpalo accent card: big Alfa Slab One
  // value over a vivid fill, charcoal outline, no shadow.
  type Tone = "pumpkin" | "sky" | "seafoam" | "lavender" | "lemon" | "spring";
  interface Props {
    value: string | number;
    label: string;
    tone?: Tone;
  }
  let { value, label, tone = "sky" }: Props = $props();

  const tones: Record<Tone, string> = {
    pumpkin: "bg-pumpkin",
    sky: "bg-sky",
    seafoam: "bg-seafoam",
    lavender: "bg-lavender",
    lemon: "bg-lemon",
    spring: "bg-spring",
  };
</script>

<div class="rounded-bubble border-2 border-charcoal {tones[tone]} px-5 py-4 text-charcoal">
  <div class="font-display text-page leading-none">{value}</div>
  <div class="mt-2 text-body font-bold">{label}</div>
</div>

<script lang="ts">
  import { tweened } from "svelte/motion";
  import { cubicOut } from "svelte/easing";
  import { healthColor } from "../lib/health";

  export let score: number;
  export let size = 96;
  // Stroke width and radius, in the 100-unit drawing: the defaults are the
  // compact ring of the advanced Summary page.
  export let thickness = 9;
  export let radius = 40;
  // Small caption under the number, inside the ring.
  export let label = "";
  // True while the first scan runs: the ring waits at zero and fills once the
  // real value is known, instead of filling for a value that is about to change.
  export let loading = false;

  const reducedMotion = typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  const shown = tweened(0, { duration: reducedMotion ? 0 : 1100, easing: cubicOut });

  let ready = false;
  $: if (!loading) ready = true;
  // Starts at zero and grows to the value; later changes slide to the new one.
  $: shown.set(ready ? score : 0);

  $: circumference = 2 * Math.PI * radius;
  $: color = healthColor(score);
  $: numberSize = Math.round(radius * 0.65);
  $: value = Math.round($shown);
</script>

<svg width={size} height={size} viewBox="0 0 100 100" class="shrink-0" role="img" aria-label={label ? `${label}: ${score}` : String(score)}>
  <circle cx="50" cy="50" r={radius} fill="none" stroke="rgb(var(--color-surface-2))" stroke-width={thickness} />
  <circle
    cx="50"
    cy="50"
    r={radius}
    fill="none"
    stroke={color}
    stroke-width={thickness}
    stroke-linecap="round"
    stroke-dasharray="{(circumference * $shown) / 100} {circumference}"
    transform="rotate(-90 50 50)"
  />
  <text x="50" y={label ? 54 : 55} text-anchor="middle" fill="rgb(var(--color-text-bright))" font-size={numberSize} font-weight="700">{value}</text>
  {#if label}
    <text x="50" y="67" text-anchor="middle" fill="rgb(var(--color-text-muted))" font-size="6.5" font-weight="600" letter-spacing="0.8">
      {label.toUpperCase()}
    </text>
  {/if}
</svg>

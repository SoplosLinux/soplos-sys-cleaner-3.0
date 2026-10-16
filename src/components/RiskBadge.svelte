<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { Risk } from "../lib/categories";

  export let risk: Risk;

  const tone: Record<Risk, string> = {
    safe: "text-success border-success/60",
    review: "text-warning border-warning/60",
    protected: "text-danger border-danger/60",
  };
</script>

<span class="inline-block whitespace-nowrap rounded-full border px-[7px] py-px text-[10.5px] {tone[risk]}">
  {$_(`page.risk.${risk}`)}
</span>

<script lang="ts">
  import { fade } from "svelte/transition";
  import { toastMessage } from "../lib/ui";
</script>

{#if $toastMessage}
  <div
    class="fixed bottom-4 left-1/2 z-[60] max-w-[80vw] -translate-x-1/2 rounded-lg bg-text-bright px-3.5 py-[7px] text-[12.5px] text-dark shadow-lg"
    role="status"
    transition:fade={{ duration: 120 }}
  >
    {$toastMessage}
  </div>
{/if}

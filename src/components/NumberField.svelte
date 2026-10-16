<script lang="ts">
  // A plain text field that only takes digits, with large step buttons. The
  // native number spinner is too small to hit, and it swallows typing in some
  // webviews.
  export let value: number | null = null;
  export let disabled = false;
  export let max = 9999;

  let text = value === null ? "" : String(value);
  $: if ((value === null ? "" : String(value)) !== text && document.activeElement?.tagName !== "INPUT") {
    text = value === null ? "" : String(value);
  }

  function onInput(event: Event) {
    const digits = (event.currentTarget as HTMLInputElement).value.replace(/\D/g, "").slice(0, 4);
    text = digits;
    (event.currentTarget as HTMLInputElement).value = digits;
    value = digits === "" ? null : Math.min(Number(digits), max);
  }

  function setDigits(digits: string) {
    text = digits;
    value = digits === "" ? null : Math.min(Number(digits), max);
  }

  // Digits and deletion are handled here instead of through the input method,
  // which can swallow key presses in some webviews (IBus under WebKitGTK).
  function onKeydown(event: KeyboardEvent) {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    if (/^\d$/.test(event.key)) {
      event.preventDefault();
      setDigits((text + event.key).slice(0, 4));
    } else if (event.key === "Backspace") {
      event.preventDefault();
      setDigits(text.slice(0, -1));
    } else if (event.key === "Delete") {
      event.preventDefault();
      setDigits("");
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      step(1);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      step(-1);
    }
  }

  function step(delta: number) {
    const next = Math.min(Math.max((value ?? 0) + delta, 1), max);
    value = next;
    text = String(next);
  }

  const button =
    "flex h-8 w-8 items-center justify-center rounded border border-border bg-surface-2 text-[16px] leading-none text-text-bright select-none hover:border-primary/60 focus-visible:ring-2 focus-visible:ring-primary/60 focus-visible:outline-none disabled:opacity-40";
</script>

<div class="inline-flex items-center gap-1">
  <button type="button" class={button} {disabled} tabindex="-1" on:click={() => step(-1)} aria-label="-1">&minus;</button>
  <input
    type="text"
    inputmode="numeric"
    autocomplete="off"
    class="h-8 w-16 rounded border border-border bg-surface-2 px-2 text-center text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60 disabled:opacity-40"
    value={text}
    {disabled}
    on:keydown={onKeydown}
    on:input={onInput}
  />
  <button type="button" class={button} {disabled} tabindex="-1" on:click={() => step(1)} aria-label="+1">+</button>
</div>

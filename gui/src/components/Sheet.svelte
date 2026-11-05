<script lang="ts">
  // A window-modal sheet (design §8.1). The default button is always the
  // cancel button: Return cancels everywhere except in text fields (where it
  // does nothing) and multi-line fields (newline). Esc cancels. While an input
  // method is composing, both keys belong to it. Confirm buttons need a click,
  // or Tab to them and Space.
  import { onMount, type Snippet } from "svelte";

  let {
    title,
    oncancel,
    cancelLabel = "取消",
    closable = true,
    note = "",
    children,
    actions,
  }: {
    title: string;
    oncancel: () => void;
    cancelLabel?: string;
    closable?: boolean;
    note?: string;
    children: Snippet;
    actions?: Snippet;
  } = $props();

  let sheet: HTMLDivElement;
  let cancel: HTMLButtonElement | undefined = $state();

  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    const veil = sheet.parentElement!;
    const siblings = [...(veil.parentElement?.children ?? [])].filter((e) => e !== veil) as HTMLElement[];
    for (const e of siblings) e.inert = true;
    (cancel ?? sheet.querySelector<HTMLElement>("button, input, textarea"))?.focus();
    return () => {
      for (const e of siblings) e.inert = false;
      previous?.focus?.();
    };
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.isComposing || e.keyCode === 229) return;
    const t = e.target as HTMLElement;
    if (e.key === "Escape") {
      e.preventDefault();
      if (closable) oncancel();
    } else if (e.key === "Enter" && !e.metaKey) {
      if (t instanceof HTMLTextAreaElement) return;
      e.preventDefault();
      const textField = t instanceof HTMLInputElement && t.type !== "checkbox";
      if (!textField && closable) oncancel();
    }
  }
</script>

<div class="veil">
  <div bind:this={sheet} class="sheet" role="dialog" aria-modal="true" aria-label={title} tabindex="-1" {onkeydown}>
    <h3>{title}</h3>
    {@render children()}
    <div class="acts">
      {#if note}<span class="muted">{note}</span>{/if}
      {#if closable}
        <button bind:this={cancel} class="btn" type="button" onclick={oncancel}>{cancelLabel}</button>
      {/if}
      {@render actions?.()}
    </div>
  </div>
</div>

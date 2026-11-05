<script lang="ts" module>
  const P: Record<string, string> = {
    overview: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
    structure: "M5 4v16M5 6h4M5 12h4M5 18h4M12 6h8M12 12h8M12 18h8",
    organize: "M4 7h13M14 4l3 3-3 3M20 17H7M10 14l-3 3 3 3",
    history: "M12 7v5l3 2M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0",
    recovery: "M12 3l9.5 17h-19zM12 10v4.5M12 17.5v.01",
    refresh: "M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7",
    plus: "M12 5v14M5 12h14",
    log: "M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01",
    insp: "M4 5h16v14H4zM15 5v14",
    folder: "M3 7.5A1.5 1.5 0 0 1 4.5 6H9l2 2h8.5A1.5 1.5 0 0 1 21 9.5v8a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z",
    file: "M6.5 3h7.5l4.5 4.5V21h-12zM14 3v4.5h4.5",
    lock: "M7.5 11V8.5a4.5 4.5 0 0 1 9 0V11M5.5 11h13v9h-13z",
    check: "M5 12.5l4.5 4.5L19 7.5",
    warn: "M12 3l9.5 17h-19zM12 10v4.5M12 17.5v.01",
    info: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v6M12 7.5v.01",
    tree: "M12 3.5v17M12 8.5l-5.5 4M12 8.5l5.5 4M12 14l-6.5 5M12 14l6.5 5",
  };
</script>

<script lang="ts">
  let { name, class: cls = "" }: { name: keyof typeof P | string; class?: string } = $props();
</script>

<svg class={cls} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={P[name] ?? ""} /></svg>

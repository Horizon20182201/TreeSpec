<script lang="ts">
  import { app } from "../stores/app.svelte";

  // `command` is built by Rust (Action::command), never assembled here.
  let { command }: { command: string | null } = $props();

  async function copy() {
    if (!command) return;
    try {
      await navigator.clipboard.writeText(command);
      app.notify("已拷贝等价命令");
    } catch {
      app.notify("无法访问剪贴板，请手动选择文本拷贝");
    }
  }
</script>

<div class="tiny muted" style="margin-top:12px">等价命令</div>
<div class="cmd" aria-label="等价命令">
  {command ?? "…"}
  <button class="btn sm" type="button" onclick={copy} disabled={!command}>拷贝</button>
</div>

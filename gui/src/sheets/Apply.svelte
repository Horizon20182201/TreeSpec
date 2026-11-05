<script lang="ts">
  // 执行迁移 (design §7.5): automatic preview, strong confirmation by typing
  // `apply`, then a progress view that cannot be dismissed.
  import Cmd from "./Cmd.svelte";
  import Icon from "../components/Icon.svelte";
  import Sheet from "../components/Sheet.svelte";
  import { archive } from "../stores/actions";
  import { app } from "../stores/app.svelte";

  const s = $derived(app.sheet);
  const v = $derived(app.view!);
  let typed = $state("");
  let executing = $state(false);
  const plan = $derived(s?.kind === "apply" || s?.kind === "apply_run" ? s.plan : "");
  const detail = $derived(app.detail?.plan?.name === plan ? app.detail.plan : null);
  const kinds = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const op of detail?.ops ?? []) counts.set(op.kind, (counts.get(op.kind) ?? 0) + 1);
    return [...counts].map(([k, n]) => `${n} ${k}`).join("，");
  });

  function execute() {
    if (s?.kind !== "apply" || typed !== "apply") return;
    executing = true;
    const token = s.token;
    app.sheet = { kind: "apply_run", plan: s.plan };
    void app.run({ kind: "apply", plan: s.plan, token }, "正在执行迁移");
  }
</script>

{#if s?.kind === "apply_run"}
  {#if executing}
    <Sheet title="正在执行迁移" closable={false} oncancel={() => {}}>
      <div class="prog"><i style:width="{app.progress ? (100 * app.progress.done) / Math.max(1, app.progress.total) : 0}%"></i></div>
      <p style="margin:0">{app.progress ? `第 ${app.progress.done} / ${app.progress.total} 步` : "正在开始…"}</p>
      <p class="muted tiny">执行期间请不要改动该目录。没有取消按钮：部分完成的迁移无法续跑，通常比等它完成更糟。关闭窗口和退出会被拦截。</p>
    </Sheet>
  {:else}
    <Sheet title="执行前预览" oncancel={() => (app.sheet = null)}>
      <div class="prog indet"><i></i></div>
      <p class="muted" style="margin:0">正在预览 plans/{plan}（只读，等价于 apply --dry-run）…</p>
    </Sheet>
  {/if}
{:else if s?.kind === "apply"}
  <Sheet title="执行迁移" oncancel={() => (app.sheet = null)} note="输入 apply 后才能执行">
    <dl class="kv">
      <dt>根目录</dt><dd class="mono">{v.root}</dd>
      <dt>计划</dt><dd class="mono">plans/{s.plan}</dd>
      <dt>操作</dt><dd>{s.ops} 个{kinds ? `：${kinds}` : ""}</dd>
      <dt>预览</dt><dd><span class="badge ok">刚刚通过</span></dd>
    </dl>
    <div class="notice"><Icon name="warn" /><span>只创建目录和移动文件，不删除、不覆盖。任一步失败立即停止，已完成的移动不会自动回滚。执行期间请不要改动该目录。</span></div>
    <Cmd action={{ kind: "apply", plan: s.plan, token: s.token }} />
    <div class="fld" style="margin-top:12px"><span>确认</span>
      <input class="inp mono" placeholder="输入 apply" aria-label="输入 apply 确认" bind:value={typed} autocomplete="off" spellcheck="false" autocapitalize="off" />
    </div>
    {#snippet actions()}<button class="btn danger" disabled={typed !== "apply" || !!app.busy} onclick={execute}>执行</button>{/snippet}
  </Sheet>
{:else if s?.kind === "apply_done"}
  {@const o = s.result.outcome}
  <Sheet title="迁移完成" cancelLabel="完成" oncancel={() => (app.sheet = null)}>
    <div class="hero ok" style="margin-bottom:10px"><div class="big"><Icon name="check" /></div><div><b>{s.result.message}</b><div class="muted tiny">文件已在目标位置，结构历史已更新。</div></div></div>
    {#if o?.kind === "applied"}<dl class="kv"><dt>run_id</dt><dd class="mono selectable">{o.run_id}</dd><dt>Git 提交</dt><dd class="mono">{o.git_head ?? "—"}</dd></dl>{/if}
    <p class="muted tiny">草案和计划仍在 drafts/、plans/ 中，会阻止同步。用完后归档。</p>
    {#snippet actions()}{#if s.round}<button class="btn pri" onclick={() => archive(s.round)}>归档本轮…</button>{/if}{/snippet}
  </Sheet>
{/if}

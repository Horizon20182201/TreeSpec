<script lang="ts">
  // 新建 LLM 草案 (design §7.3): privacy confirmation. Nothing is sent until
  // “发送” is clicked; Return inserts a newline in the request.
  import { onDestroy } from "svelte";
  import Cmd from "./Cmd.svelte";
  import Sheet from "../components/Sheet.svelte";
  import { api } from "../ipc/api";
  import type { ScopeView } from "../ipc/bindings/ScopeView";
  import { bytes, chatName } from "../lib/format";
  import { app } from "../stores/app.svelte";

  const v = $derived(app.view!);
  let prompt = $state("");
  let classify = $state(true);
  let readContent = $state(false);
  let out = $state(chatName());
  let preview = $state<ScopeView | null>(null);
  let invalid = $state<string | null>(null);
  let now = $state(Date.now());
  const timer = setInterval(() => (now = Date.now()), 500);
  onDestroy(() => clearInterval(timer));

  const size = $derived(bytes(prompt));
  const scopeNames = $derived(app.scope.map((id) => v.nodes.find((n) => n.id === id)?.path ?? id));
  const action = $derived({ kind: "chat" as const, prompt, scope: [...app.scope], classify, read_content: readContent && classify, out });
  const ready = $derived(prompt.trim().length > 0 && size <= 8192 && !!preview?.ok && !invalid && !app.busy);

  $effect(() => {
    api.previewScope([...app.scope]).then((p) => (preview = p), () => (preview = null));
  });

  function send() {
    app.sheet = { kind: "chat_wait", started: Date.now() };
    void app.run(action, "正在生成草案");
  }
</script>

{#if app.sheet?.kind === "chat_wait"}
  <Sheet title="正在生成草案" closable={false} oncancel={() => {}}>
    <div class="prog indet"><i></i></div>
    <p class="muted" style="margin:0">已用 {Math.round((now - app.sheet.started) / 1000)} 秒。一次请求最多调用模型 4 次，每次最长 30 秒；等待期间不能取消。窗口其他部分仍可浏览。</p>
  </Sheet>
{:else}
  <Sheet title="新建 LLM 草案" oncancel={() => (app.sheet = null)} note="“发送”不是默认按钮">
    <div class="fld"><span>请求</span><span>
      <textarea class="inp" aria-label="请求" bind:value={prompt} placeholder="例如：在 Papers 下按主题建子目录并归类论文"></textarea>
      <span class="hint" style:color={size > 8192 ? "var(--bad)" : ""}>{size} / 8192 字节 · Return 换行，不会发送</span>
    </span></div>
    <div class="fld"><span>范围</span><span>{scopeNames.length ? scopeNames.join("、") : "整个工作区"} <button class="btn sm" onclick={() => ((app.sheet = null), (app.page = "structure"))}>更改…</button></span></div>
    <label class="ck"><input type="checkbox" bind:checked={classify} /><span>允许重新分类已有文件 <span class="mono muted">--classify</span></span></label>
    <label class="ck"><input type="checkbox" bind:checked={readContent} disabled={!classify || v.llm.no_content} />
      <span class:muted={v.llm.no_content}>发送文本片段（单文件 4 KiB，整次 64 KiB） <span class="mono muted">--read-content</span>
        {#if v.llm.no_content}<br /><span class="tiny">已被“禁止发送文本片段”关闭</span>{/if}</span></label>
    <div class="fld"><span>输出</span><span class="mono">drafts/<input class="inp mono" style="width:220px;display:inline-block" aria-label="输出文件名" bind:value={out} spellcheck="false" /></span></div>
    <dl class="kv" style="margin-top:12px">
      <dt>将发送到</dt><dd>{v.llm.host ?? "?"}（{v.llm.remote ? "远程，HTTPS" : "本地"}）</dd>
      <dt>发送内容</dt>
      <dd>
        {#if !preview}…
        {:else if !preview.ok}<span style="color:var(--bad)">{preview.error}</span>
        {:else}范围内 {preview.visible_nodes} 个节点的名称、相对路径、ID、说明；{readContent && classify ? "以及受支持文本文件的有限片段（PDF 和二进制文件只用文件名）" : "不含文件正文"}；{preview.excluded} 个敏感或受保护节点已排除{/if}
        {#if preview?.warn_files}<br /><span class="badge warn">范围内文件超过 256 个，分类可能超限</span>{/if}
      </dd>
    </dl>
    <Cmd {action} onerror={(e) => (invalid = e)} />
    {#snippet actions()}<button class="btn pri" disabled={!ready} onclick={send}>发送</button>{/snippet}
  </Sheet>
{/if}

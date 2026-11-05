// Confirmation keyboard rules (design §8.1): Return and Esc cancel, Return in
// a text field does nothing, and an input method's composition is left alone.
import { fireEvent, render } from "@testing-library/svelte";
import { createRawSnippet } from "svelte";
import { describe, expect, it, vi } from "vitest";
import Sheet from "../../src/components/Sheet.svelte";

function sheet(extra: Record<string, unknown> = {}) {
  const oncancel = vi.fn();
  const children = createRawSnippet(() => ({ render: () => `<div><input type="text" aria-label="名称" /><textarea aria-label="请求"></textarea><input type="checkbox" aria-label="勾选" /></div>` }));
  const actions = createRawSnippet(() => ({ render: () => `<button class="btn pri">执行</button>` }));
  const r = render(Sheet, { props: { title: "测试", oncancel, children, actions, ...extra } });
  return { ...r, oncancel };
}

describe("Sheet", () => {
  it("focuses cancel and treats Return as cancel", async () => {
    const { getByRole, oncancel } = sheet();
    const cancel = getByRole("button", { name: "取消" });
    expect(document.activeElement).toBe(cancel);
    await fireEvent.keyDown(cancel, { key: "Enter" });
    expect(oncancel).toHaveBeenCalledTimes(1);
    // Return on the confirm button still cancels; confirming needs a click or Space.
    await fireEvent.keyDown(getByRole("button", { name: "执行" }), { key: "Enter" });
    expect(oncancel).toHaveBeenCalledTimes(2);
  });

  it("does nothing on Return in text fields or while composing", async () => {
    const { getByLabelText, oncancel } = sheet();
    await fireEvent.keyDown(getByLabelText("名称"), { key: "Enter" });
    await fireEvent.keyDown(getByLabelText("请求"), { key: "Enter" });
    await fireEvent.keyDown(getByLabelText("勾选"), { key: "Enter", isComposing: true });
    await fireEvent.keyDown(getByLabelText("名称"), { key: "Escape", isComposing: true });
    expect(oncancel).not.toHaveBeenCalled();
    await fireEvent.keyDown(getByLabelText("名称"), { key: "Escape" });
    expect(oncancel).toHaveBeenCalledTimes(1);
  });

  it("cannot be dismissed while not closable", async () => {
    const { queryByRole, getByRole, oncancel } = sheet({ closable: false });
    expect(queryByRole("button", { name: "取消" })).toBeNull();
    await fireEvent.keyDown(getByRole("dialog"), { key: "Escape" });
    await fireEvent.keyDown(getByRole("dialog"), { key: "Enter" });
    expect(oncancel).not.toHaveBeenCalled();
  });
});

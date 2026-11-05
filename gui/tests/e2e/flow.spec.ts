// End-to-end flows against the in-memory backend (`--mode mock`). They cover
// the page logic and keyboard rules; the real backend is tested in Rust.
import { expect, test } from "@playwright/test";

test("plan, preview, strong confirmation, apply and archive", async ({ page }) => {
  await page.goto("/?scenario=clean");
  await page.getByRole("button", { name: "确认并生成计划…" }).click();
  const sheet = page.getByRole("dialog");
  await expect(sheet.getByRole("button", { name: "生成计划" })).toBeDisabled();
  await page.keyboard.press("Enter");
  await expect(sheet).toHaveCount(0);

  await page.getByRole("button", { name: "确认并生成计划…" }).click();
  await sheet.getByRole("checkbox").check();
  await sheet.getByRole("button", { name: "生成计划" }).click();
  await expect(page.locator(".rounds")).toContainText("可执行");

  await page.locator(".detail").getByRole("button", { name: "执行迁移…" }).click();
  const confirm = page.getByLabel("输入 apply 确认");
  await confirm.fill("appl");
  await expect(sheet.getByRole("button", { name: "执行" })).toBeDisabled();
  await confirm.fill("apply");
  await confirm.press("Enter");
  await expect(sheet.getByRole("button", { name: "执行" })).toBeEnabled();
  await sheet.getByRole("button", { name: "执行" }).click();
  await expect(page.getByRole("dialog", { name: "正在执行迁移" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "迁移完成" })).toBeVisible({ timeout: 8000 });
  await page.getByRole("dialog", { name: "迁移完成" }).getByRole("button", { name: "归档本轮…" }).click();
  await page.getByRole("button", { name: /^归档 \d+ 个文件$/ }).click();
  await expect(page.locator(".rounds")).not.toContainText("papers.tree");
  await page.getByRole("button", { name: "活动日志" }).click();
  await expect(page.locator(".log")).toContainText("organizer apply");
});

test("disk changes block new drafts until archived and synced", async ({ page }) => {
  await page.goto("/?scenario=changed");
  await expect(page.getByRole("button", { name: "新建草案", exact: true })).toBeDisabled();
  await expect(page.locator(".hero").getByRole("button", { name: "同步…" })).toBeDisabled();
  await page.locator(".hero").getByRole("button", { name: "全部归档…" }).click();
  await page.getByRole("button", { name: /^归档 \d+ 个文件$/ }).click();
  await page.locator(".hero").getByRole("button", { name: "同步…" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "同步" }).click();
  await expect(page.locator(".pill")).toContainText("已同步");
  await expect(page.getByRole("button", { name: "新建草案", exact: true })).toBeEnabled();
});

test("an unfinished migration opens recovery and locks other writes", async ({ page }) => {
  await page.goto("/?scenario=unfinished");
  await expect(page.getByRole("heading", { name: "run_7b2e41c9" })).toBeVisible();
  await expect(page.getByRole("button", { name: "补记 Git…" })).toBeDisabled();
  await page.getByRole("button", { name: "标记已人工处理…" }).click();
  const mark = page.getByRole("dialog").getByRole("button", { name: "标记已处理" });
  await expect(mark).toBeDisabled();
  await page.getByRole("dialog").getByRole("checkbox").check();
  await mark.click();
  await expect(page.locator(".pill")).toContainText("磁盘有变化");
});

test("the editor guards unsaved changes", async ({ page }) => {
  await page.goto("/?scenario=clean");
  await page.getByRole("button", { name: "整理" }).click();
  await page.locator(".rd", { hasText: "inbox.tree" }).click();
  await page.getByRole("tab", { name: "编辑" }).click();
  await page.locator(".cm-content").click();
  await page.keyboard.press("Control+End");
  await page.keyboard.type("   odd");
  await expect(page.locator(".chk")).toContainText("第 15 行");
  await page.getByRole("button", { name: "结构" }).click();
  await expect(page.getByRole("dialog", { name: "草案有未保存的修改" })).toBeVisible();
  await page.getByRole("button", { name: "不保存" }).click();
  await expect(page.getByRole("heading", { name: "基线结构" })).toBeVisible();
});

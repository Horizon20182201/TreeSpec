// Untrusted names, descriptions and errors render as text only (design §8.2).
import { render } from "@testing-library/svelte";
import { tick } from "svelte";
import { beforeAll, describe, expect, it } from "vitest";
import { api } from "../../src/ipc/api";
import { installMock } from "../../src/mock/backend";
import Overview from "../../src/pages/Overview.svelte";
import Structure from "../../src/pages/Structure.svelte";
import { app } from "../../src/stores/app.svelte";

const EVIL = `<img src=x onerror="window.__pwned=1"></script><b>x</b>‮`;

beforeAll(() => installMock("changed"));

describe("untrusted text", () => {
  it("never creates elements from names, notes or errors", async () => {
    const view = (await api.snapshot())!;
    view.nodes[0] = { ...view.nodes[0], name: EVIL, description: EVIL };
    view.overview!.added = [{ path: EVIL, old_path: null, note: EVIL }];
    view.overview!.conflicts = [EVIL];
    view.rounds[0] = { ...view.rounds[0], name: EVIL };
    app.setView(view);
    const overview = render(Overview);
    (overview.container.querySelector(".tile") as HTMLButtonElement).click();
    await tick();
    const structure = render(Structure);
    await tick();
    for (const c of [overview.container, structure.container]) {
      expect(c.querySelector("img")).toBeNull();
      expect(c.querySelector("b > b, td b, .n b")).toBeNull();
      expect(c.textContent).toContain("<img src=x");
    }
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it("clears session-only previews on every new view", async () => {
    app.previews = { "a.json": "10:45" };
    app.handle({ kind: "view", view: (await api.snapshot())! });
    expect(app.previews).toEqual({});
  });
});

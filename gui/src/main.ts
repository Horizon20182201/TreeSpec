import "./styles/app.css";
import { mount } from "svelte";
import App from "./App.svelte";

async function start() {
  // Browser previews and Playwright tests use an in-memory backend; the Tauri
  // build is made without `--mode mock` and never contains it.
  if (import.meta.env.VITE_MOCK === "1") {
    const { installMock } = await import("./mock/backend");
    installMock(new URLSearchParams(location.search).get("scenario") ?? "clean");
  }
  mount(App, { target: document.getElementById("app")! });
}

void start();

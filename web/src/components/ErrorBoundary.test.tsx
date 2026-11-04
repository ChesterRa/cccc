// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { ErrorBoundary } from "./ErrorBoundary";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
afterEach(() => vi.restoreAllMocks());

it("puts a modal error over the application and lets the user retry", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let failed = true;
  function Child() {
    if (failed) throw new Error("fixture modal failed");
    return <div>Recovered</div>;
  }
  await act(async () =>
    root.render(
      <ErrorBoundary overlay>
        <Child />
      </ErrorBoundary>,
    ),
  );
  const alert = host.querySelector<HTMLElement>('[role="alert"]');
  expect(alert?.classList.contains("fixed")).toBe(true);
  expect(alert?.textContent).toContain("fixture modal failed");
  failed = false;
  await act(async () => alert?.querySelector("button")?.click());
  expect(host.textContent).toContain("Recovered");
  await act(async () => root.unmount());
  host.remove();
});

it("keeps a page error in its existing layout", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  function Child(): never {
    throw new Error("fixture page failed");
  }
  await act(async () =>
    root.render(
      <ErrorBoundary>
        <Child />
      </ErrorBoundary>,
    ),
  );
  const alert = host.querySelector<HTMLElement>('[role="alert"]');
  expect(alert?.classList.contains("fixed")).toBe(false);
  expect(alert?.classList.contains("h-full")).toBe(true);
  await act(async () => root.unmount());
  host.remove();
});

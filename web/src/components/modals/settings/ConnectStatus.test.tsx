// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { ConnectStatus } from "./ConnectStatus";

const mocks = vi.hoisted(() => ({ request: vi.fn() }));
vi.mock("../../../services/api/base", () => ({ apiJson: mocks.request }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const confirmed = (publicOrigin: string | null = "https://b.test") => ({
  ok: true,
  result: {
    connect: {
      instance_id: "b",
      device_id: "binding-b",
      account_origin: "https://account.test",
      checked_at: new Date().toISOString(),
      error_code: null,
      error_message: null,
      directory: {
        expires_at: new Date(Date.now() + 120000).toISOString(),
        instances: [
          {
            instance_id: "b",
            device_id: "binding-b",
            display_name: "Test B",
            public_origin: publicOrigin,
          },
        ],
      },
    },
  },
});
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  mocks.request.mockReset().mockResolvedValue(confirmed());
  host = document.createElement("div");
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  vi.useRealTimers();
});
async function render(active = true, deviceId = "binding-b") {
  await act(async () =>
    root.render(
      <ConnectStatus key={deviceId} active={active} deviceId={deviceId} refreshedAt="" />,
    ),
  );
}
it("reads cached confirmation while open without starting a connection and stops when hidden", async () => {
  await render(false);
  expect(mocks.request).not.toHaveBeenCalled();
  await render();
  expect(host.textContent).toContain("account.connect.confirmed");
  expect(host.textContent).toContain("account.connect.editingInstance");
  expect(host.textContent).toContain("b.test");
  await act(async () => vi.advanceTimersByTimeAsync(15000));
  expect(mocks.request).toHaveBeenCalledTimes(2);
  expect(
    mocks.request.mock.calls.every(
      ([path, options]) => path === "/api/v1/connect" && !options.method,
    ),
  ).toBe(true);
  await render(false);
  await act(async () => vi.advanceTimersByTimeAsync(30000));
  expect(mocks.request).toHaveBeenCalledTimes(2);
});
it("distinguishes a registered instance with no route from a confirmed collaboration route", async () => {
  mocks.request.mockResolvedValue(confirmed(null));
  await render();
  expect(host.textContent).toContain("account.connect.noRoute");
  expect(host.textContent).not.toContain("account.connect.confirmed");
});
it("shows a version rejection and stops claiming confirmation after a read fails", async () => {
  const response = confirmed();
  mocks.request.mockResolvedValue({
    ok: true,
    result: {
      connect: {
        ...response.result.connect,
        directory: null,
        error_code: "membership_unsupported_version",
        error_message: "Update to the required version.",
      },
    },
  });
  await render();
  expect(host.textContent).toContain("account.connect.upgrade");
  expect(host.textContent).toContain("Update to the required version.");
  mocks.request.mockResolvedValue({ ok: false });
  await act(async () => vi.advanceTimersByTimeAsync(15000));
  expect(host.textContent).toContain("account.connect.unavailable");
  expect(host.textContent).not.toContain("account.connect.upgrade");
});
it("does not display an old binding's late result after reconnecting the account", async () => {
  let finish!: (value: ReturnType<typeof confirmed>) => void;
  mocks.request.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await render();
  mocks.request.mockResolvedValue({ ok: true, result: { connect: null } });
  await render(true, "new-binding");
  await act(async () => finish(confirmed()));
  expect(host.textContent).toContain("account.connect.pending");
  expect(host.textContent).not.toContain("account.connect.confirmed");
});

it("keeps an edited name across refresh and saves to the shared instance naming port", async () => {
  await render();
  const input = host.querySelector("input")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
      input,
      "Workstation",
    );
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => vi.advanceTimersByTimeAsync(15000));
  expect(input.value).toBe("Workstation");
  mocks.request.mockImplementation(async (path) => {
    if (path === "/api/v1/connect/name")
      return { ok: true, result: { display_name: "Workstation" } };
    const result = confirmed();
    result.result.connect.directory.instances[0].display_name = "Workstation";
    return result;
  });
  await act(async () =>
    host
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(
    mocks.request.mock.calls.some(
      ([path, options]) =>
        path === "/api/v1/connect/name" &&
        options.method === "POST" &&
        JSON.parse(options.body).display_name === "Workstation",
    ),
  ).toBe(true);
  expect(host.querySelector("input")!.value).toBe("Workstation");
  expect(host.querySelector("button")!.disabled).toBe(true);
});

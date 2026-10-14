// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

const mocks = vi.hoisted(() => ({ connectors: vi.fn(), dots: vi.fn() }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, string>) =>
      options ? `${key}(${Object.values(options).join(",")})` : key,
    i18n: { language: "en" },
  }),
}));
vi.mock("../../../services/api", () => ({
  fetchWebModelConnectors: () => mocks.connectors(),
  fetchDotsActorState: (...args: unknown[]) => mocks.dots(...args),
}));
import DotsSettingsTab from "./DotsSettingsTab";

const connector = {
  connector_id: "wmc_1",
  provider: "chatgpt_web",
  revoked: false,
  dots_actor: { group_id: "g_a", actor_id: "nn" },
  events_log: [
    {
      at: "2026-10-12T02:00:00Z",
      method: "events/subscribe",
      ok: false,
      reason: "challenge_failed",
    },
    { at: "2026-10-12T03:00:00Z", method: "events/subscribe", ok: true, reason: null },
  ],
};
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  mocks.connectors.mockResolvedValue({
    ok: true,
    result: { connectors: [connector], connector_base_url: "https://cccc.example.test" },
  });
  mocks.dots.mockResolvedValue({
    ok: true,
    result: {
      group_id: "g_a",
      actor_id: "nn",
      enabled: true,
      connected: true,
      subscribed: true,
      phase: "ready",
      pending_count: 0,
      subscription_expires_at: null,
    },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
const render = async (props: Partial<React.ComponentProps<typeof DotsSettingsTab>> = {}) => {
  await act(async () =>
    root.render(<DotsSettingsTab isActive onOpenChatGPT={vi.fn()} {...props} />),
  );
};

it("shows the bound Actor's subscription and the newest subscription outcome first", async () => {
  await render();
  expect(mocks.dots).toHaveBeenCalledWith("g_a", "nn");
  const actor = host.querySelector('[data-dots-actor="nn"]')!;
  expect(actor.textContent).toContain("dotsActor.phases.ready");
  expect(actor.textContent).toContain("dotsActor.status.permanent");
  const calls = [...host.querySelectorAll("[data-dots-call]")];
  expect(calls.map((call) => call.getAttribute("data-dots-call"))).toEqual(["ok", "failed"]);
  expect(calls[1].textContent).toContain("dotsSettings.callFailed(challenge_failed)");
});

it("shows pairing failures in shared diagnostics without changing the Actor's ready state", async () => {
  mocks.connectors.mockResolvedValue({
    ok: true,
    result: {
      connectors: [
        {
          ...connector,
          events_log: [
            {
              at: "2026-10-14T02:00:00Z",
              method: "tools/call",
              tool_name: "cccc_pair",
              ok: false,
              reason: "pairing_expired",
            },
          ],
        },
      ],
      connector_base_url: "https://cccc.example.test",
    },
  });
  await render();
  const call = host.querySelector('[data-dots-call="failed"]')!;
  expect(call.textContent).toContain("cccc_pair");
  expect(call.textContent).toContain("dotsSettings.callFailed(pairing_expired)");
  expect(call.textContent).toContain("dotsSettings.reasons.pairingExpired");
  expect(host.querySelector('[data-dots-actor="nn"]')!.textContent).toContain(
    "dotsActor.phases.ready",
  );
});

it("never offers to create a second connector; it sends the owner to the ChatGPT view", async () => {
  const onOpenChatGPT = vi.fn();
  mocks.connectors.mockResolvedValue({
    ok: true,
    result: { connectors: [], connector_base_url: "" },
  });
  await render({ onOpenChatGPT });
  expect(host.textContent).toContain("dotsSettings.connectorMissing");
  expect(host.textContent).toContain("webModelShared.publicUrlMissing");
  expect(host.textContent).toContain("dotsSettings.noActor");
  expect(mocks.dots).not.toHaveBeenCalled();
  const buttons = [...host.querySelectorAll("button")].map((button) => button.textContent);
  expect(buttons).toEqual(["dotsSettings.manageConnector"]);
  await act(async () => (host.querySelector("button") as HTMLButtonElement).click());
  expect(onOpenChatGPT).toHaveBeenCalledOnce();
});

it("reads nothing while the Dots view is hidden", async () => {
  await render({ isActive: false });
  expect(mocks.connectors).not.toHaveBeenCalled();
});

it("never reports an unreadable Actor as unsubscribed and keeps its last trusted status", async () => {
  vi.useFakeTimers();
  try {
    await render();
    expect(host.textContent).toContain("dotsActor.status.permanent");
    mocks.dots.mockResolvedValue({ ok: false, error: { code: "x", message: "read failed" } });
    await act(async () => vi.advanceTimersByTimeAsync(5_000));
    expect(host.textContent).toContain("dotsSettings.actorReadFailed");
    expect(host.textContent).toContain("dotsActor.status.permanent");
    expect(host.textContent).not.toContain("dotsActor.status.notSubscribed");
  } finally {
    vi.useRealTimers();
  }
});

it("shows the status as unknown when the very first Actor read fails", async () => {
  mocks.dots.mockRejectedValue(new Error("offline"));
  await render();
  expect(host.textContent).toContain("dotsSettings.actorReadFailed");
  expect(host.textContent).toContain("dotsActor.unavailable");
  expect(host.textContent).not.toContain("dotsActor.status.notSubscribed");
});

it("does not start a new read while one is pending, so a late reply cannot roll status back", async () => {
  vi.useFakeTimers();
  try {
    let release!: (value: unknown) => void;
    mocks.dots.mockReturnValueOnce(new Promise((resolve) => (release = resolve)));
    await render();
    await act(async () => vi.advanceTimersByTimeAsync(12_000));
    expect(mocks.connectors).toHaveBeenCalledOnce();
    await act(async () =>
      release({
        ok: true,
        result: {
          group_id: "g_a",
          actor_id: "nn",
          enabled: true,
          connected: true,
          subscribed: false,
          phase: "waiting_subscription",
          pending_count: 0,
        },
      }),
    );
    await act(async () => vi.advanceTimersByTimeAsync(5_000));
    // The next read starts only now and its newer status wins.
    expect(mocks.connectors).toHaveBeenCalledTimes(2);
    expect(host.querySelector('[data-dots-actor="nn"]')!.textContent).toContain(
      "dotsActor.phases.ready",
    );
  } finally {
    vi.useRealTimers();
  }
});

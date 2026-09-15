// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { DirectConnectionsPanel } from "./DirectConnectionsPanel";
const mocks = vi.hoisted(() => ({ request: vi.fn(), copy: vi.fn() }));
vi.mock("../../utils/copy", () => ({ copyTextToClipboard: mocks.copy }));
vi.mock("../../services/api/base", () => ({ apiJson: mocks.request }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
let value: Record<string, unknown>;
beforeEach(() => {
  mocks.request.mockReset();
  mocks.copy.mockReset().mockResolvedValue(true);
  value = {
    display_name: "Office",
    listener: null,
    runtime: null,
    relations: [],
    addresses: [
      { address: "192.168.1.10:8847", bind: "0.0.0.0:8847", interface: "Ethernet" },
      { address: "[fd00::2]:8847", bind: "[::]:8847", interface: "VPN" },
    ],
  };
  mocks.request.mockImplementation(async (_url, init) =>
    init?.method === "POST" ? { ok: true, result: {} } : { ok: true, result: value },
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

it("clears a recovered polling error and treats cached online status as unconfirmed on failure", async () => {
  vi.useFakeTimers();
  value.relations = [
    {
      id: "direct-id",
      remote: null,
      state: "active",
      initiated: true,
      current: true,
      expired: false,
      online: true,
    },
  ];
  await render();
  expect(host.textContent).toContain("direct.states.online");
  mocks.request.mockResolvedValueOnce({
    ok: false,
    error: { code: "network_error", message: "offline" },
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(host.textContent).toContain("direct.statusUnavailable");
  expect(host.textContent).toContain("direct.states.active");
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(host.textContent).not.toContain("direct.statusUnavailable");
  expect(host.textContent).toContain("direct.states.online");
});
const button = (label: string) =>
  Array.from(host.querySelectorAll("button")).find((button) => button.textContent === label)!;
const render = async () => {
  await act(async () => root.render(<DirectConnectionsPanel groupId="group-a" />));
};
it("does not mutate on initial status polling or claim a listener is ready from configuration", async () => {
  value.listener = { bind: "0.0.0.0:8847", address: "office:8847" };
  await render();
  expect(host.textContent).toContain("direct.listenerPending");
  await act(async () => button("direct.inviteGroup").click());
  expect(button("direct.create").disabled).toBe(false);
  expect(mocks.request.mock.calls.every(([, init]) => !init?.method)).toBe(true);
});
it("creates an invitation for this Group only after verified listener status", async () => {
  const text = invitation();
  value.listener = { bind: "0.0.0.0:8847", address: "office:8847" };
  value.runtime = { listener: true, error: null };
  mocks.request.mockImplementation(async (_url, init) =>
    init?.method === "POST"
      ? { ok: true, result: { invitation: text } }
      : { ok: true, result: value },
  );
  await render();
  await act(async () => button("direct.inviteGroup").click());
  await act(async () => button("direct.create").click());
  expect(
    JSON.parse(mocks.request.mock.calls.find(([, init]) => init?.method === "POST")![1].body),
  ).toEqual({ action: "invite", group_id: "group-a", expected_listener: value.listener });
  expect(host.querySelector<HTMLTextAreaElement>("textarea[readonly]")?.value).toBe(text);
});
it("requires explicit approval of the requesting pair and never presents pending as connected", async () => {
  value.relations = [
    {
      id: "direct-id",
      local: { title: "A" },
      remote: { name: "Office B", title: "Build", instance_id: "peer-b" },
      state: "pending",
      initiated: false,
      current: true,
      expired: false,
      online: false,
      error: null,
    },
  ];
  await render();
  expect(host.textContent).toContain("Office B · Build");
  expect(host.textContent).toContain("direct.states.needsApproval");
  await act(async () => button("direct.approve").click());
  expect(mocks.request.mock.calls.some(([, init]) => init?.method === "POST")).toBe(false);
  expect(host.textContent).toContain("direct.approveHint");
  await act(async () => button("direct.confirm").click());
  expect(
    JSON.parse(mocks.request.mock.calls.find(([, init]) => init?.method === "POST")![1].body),
  ).toEqual({ action: "approve", group_id: "group-a", id: "direct-id" });
});
it("offers removal for expired invitations but cannot approve a replaced Group", async () => {
  value.relations = [
    {
      id: "direct-id",
      remote: null,
      state: "pending",
      initiated: false,
      current: false,
      expired: true,
      online: false,
    },
  ];
  await render();
  expect(host.textContent).toContain("direct.states.replaced");
  expect(button("direct.approve")).toBeUndefined();
  expect(button("direct.remove")).toBeDefined();
});

function invitation(expires = Date.now() + 1800000) {
  return `cccc-direct:${btoa(
    String.fromCharCode(
      ...new TextEncoder().encode(
        JSON.stringify({
          v: 1,
          id: "direct-fixture",
          address: "office.test:8847",
          secret: "a".repeat(64),
          expires_at: new Date(expires).toISOString(),
          host: {
            instance_id: "peer-a",
            name: "办公室",
            group_id: "remote-group",
            title: "Review",
          },
        }),
      ),
    ),
  )}`;
}
function relation(overrides: Record<string, unknown> = {}) {
  return {
    id: "direct-fixture",
    local: { title: "Local A" },
    remote: null,
    state: "invited",
    initiated: false,
    current: true,
    expired: false,
    online: false,
    error: null,
    expires_at: new Date(Date.now() + 1800000).toISOString(),
    ...overrides,
  };
}
async function fill(field: HTMLInputElement | HTMLTextAreaElement, text: string) {
  await act(async () => {
    const proto =
      field instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(field, text);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const postBodies = () =>
  mocks.request.mock.calls
    .filter(([, init]) => init?.method === "POST")
    .map(([, init]) => JSON.parse(init.body));

it("keeps an expired pending join visible until the receiver confirms a decision", async () => {
  vi.useFakeTimers();
  value.relations = [relation({ state: "pending", initiated: true, expired: true })];
  await render();
  expect(host.textContent).toContain("direct.states.checkingApproval");
  expect(host.textContent).toContain("direct.checkingApprovalHint");
  expect(host.textContent).not.toContain("direct.history");
  expect(button("direct.remove")).toBeUndefined();
  expect(button("direct.cancelRequest")).toBeDefined();
  value.relations = [relation({ state: "active", initiated: true, expired: true, online: true })];
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(host.textContent).toContain("direct.states.online");
  expect(host.textContent).not.toContain("direct.states.checkingApproval");
  value.relations = [relation({ state: "expired", initiated: true, expired: true })];
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(host.textContent).toContain("direct.states.expired");
  expect(host.textContent).toContain("direct.history");
  expect(button("direct.remove")).toBeDefined();
  expect(button("direct.cancelRequest")).toBeUndefined();
  expect(postBodies()).toEqual([]);
});

it("shows initial loading without asserting that receiving is off", async () => {
  mocks.request.mockImplementation(() => new Promise(() => {}));
  await render();
  expect(host.textContent).toContain("direct.loading");
  expect(host.textContent).not.toContain("direct.listenerOff");
  expect(button("direct.inviteGroup").disabled).toBe(true);
});
it("creates from the detected address with one confirmation and waits for readiness", async () => {
  vi.useFakeTimers();
  mocks.request.mockImplementation(async (_url, init) => {
    if (init?.method === "POST") {
      const request = JSON.parse(init.body);
      if (request.action === "configure") {
        value.listener = request.listener;
        return { ok: true, result: { configured: true } };
      }
      return { ok: true, result: { invitation: invitation() } };
    }
    return { ok: true, result: { ...value } };
  });
  await render();
  await act(async () => button("direct.inviteGroup").click());
  expect(host.querySelector("input[placeholder]")).toBeNull();
  expect(host.textContent).toContain("192.168.1.10:8847");
  expect(host.textContent).toContain("direct.createEffect");
  expect(postBodies()).toEqual([]);
  await act(async () => button("direct.create").click());
  expect(postBodies()).toEqual([
    {
      action: "configure",
      group_id: "group-a",
      listener: { bind: "0.0.0.0:8847", address: "192.168.1.10:8847" },
      display_name: "Office",
      expected_listener: null,
    },
  ]);
  expect(host.textContent).toContain("direct.preparing");
  expect(button("direct.creating").disabled).toBe(true);
  // No second click is needed after the daemon confirms the listener.
  value.runtime = { listener: true, error: null };
  await act(async () => {
    await vi.advanceTimersByTimeAsync(500);
  });
  expect(postBodies().map((b) => b.action)).toEqual(["configure", "invite"]);
  expect(host.querySelector<HTMLTextAreaElement>("textarea[readonly]")?.value).toContain(
    "cccc-direct:",
  );
});
it("previews the invited pair, blocks expired content and joins without configuring a listener", async () => {
  await render();
  await act(async () => button("direct.useInvite").click());
  const input = host.querySelector("textarea")!;
  await fill(input, "not an invitation");
  expect(button("direct.request").disabled).toBe(true);
  await fill(input, invitation(Date.now() - 100));
  expect(host.textContent).toContain("direct.expiredHint");
  expect(button("direct.request").disabled).toBe(true);
  const text = invitation();
  await fill(input, text);
  expect(host.textContent).toContain("group-a ↔ 办公室 · Review");
  expect(host.textContent).toContain("office.test:8847");
  expect(postBodies()).toHaveLength(0);
  await act(async () => button("direct.request").click());
  expect(postBodies()).toEqual([{ action: "join", group_id: "group-a", invitation: text }]);
});
it("keeps a confirmed invitation copyable on a failed refresh and retires the copy with its relation", async () => {
  vi.useFakeTimers();
  const text = invitation();
  value.listener = { bind: "0.0.0.0:8847", address: "office:8847" };
  value.runtime = { listener: true, error: null };
  let failPoll = false;
  mocks.request.mockImplementation(async (_url, init) => {
    if (init?.method === "POST") {
      failPoll = true;
      return { ok: true, result: { invitation: text } };
    }
    return failPoll
      ? { ok: false, error: { message: "offline" } }
      : { ok: true, result: { ...value } };
  });
  await render();
  await act(async () => button("direct.inviteGroup").click());
  await act(async () => button("direct.create").click());
  expect(host.querySelector<HTMLTextAreaElement>("textarea[readonly]")?.value).toBe(text);
  await act(async () => button("direct.copy").click());
  expect(mocks.copy).toHaveBeenCalledWith(text);
  expect(host.textContent).toContain("direct.copied");
  failPoll = false;
  value.relations = [relation()];
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(host.querySelectorAll("textarea[readonly]")).toHaveLength(1);
  value.relations = [];
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(host.querySelectorAll("textarea[readonly]")).toHaveLength(0);
});
it("explains closed-panel invitation recovery without fetching or regenerating a secret", async () => {
  value.relations = [relation()];
  await render();
  expect(host.textContent).toContain("direct.invitationLost");
  expect(button("direct.cancelInvite")).toBeDefined();
  expect(host.querySelector("textarea[readonly]")).toBeNull();
  expect(postBodies()).toEqual([]);
});
it("does not keep the saved-request notice after authoritative approval", async () => {
  vi.useFakeTimers();
  mocks.request.mockImplementation(async (_url, init) =>
    init?.method === "POST"
      ? { ok: true, result: { id: "direct-fixture" } }
      : { ok: true, result: { ...value } },
  );
  await render();
  await act(async () => button("direct.useInvite").click());
  await fill(host.querySelector("textarea")!, invitation());
  await act(async () => button("direct.request").click());
  expect(host.textContent).toContain("direct.requestSubmitted");
  value.relations = [relation({ state: "active", online: true, initiated: true })];
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(host.textContent).toContain("direct.states.online");
  expect(host.textContent).not.toContain("direct.requestSubmitted");
  value.relations = [];
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(host.textContent).not.toContain("direct.requestSubmitted");
});

it("expires a pasted invitation while the panel remains open", async () => {
  vi.useFakeTimers();
  await render();
  await act(async () => button("direct.useInvite").click());
  await fill(host.querySelector("textarea")!, invitation(Date.now() + 1000));
  expect(button("direct.request").disabled).toBe(false);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(button("direct.request").disabled).toBe(true);
  expect(postBodies()).toHaveLength(0);
});
it("requires confirmation before stopping the shared receiver and leaves unsaved names alone", async () => {
  value.listener = { bind: "0.0.0.0:8847", address: "office:8847" };
  value.runtime = { listener: true, error: null };
  await render();
  await act(async () => button("direct.configure").click());
  await fill(host.querySelector<HTMLInputElement>('input[maxlength="60"]')!, "Unsaved name");
  await act(async () => button("direct.stop").click());
  expect(postBodies()).toEqual([]);
  expect(host.textContent).toContain("direct.stopHint");
  await act(async () => button("direct.stopConfirm").click());
  expect(postBodies()).toEqual([
    { action: "configure", group_id: "group-a", listener: null, expected_listener: value.listener },
  ]);
});
it("rechecks an uncertain request without retrying the mutation", async () => {
  vi.useFakeTimers();
  value.listener = { bind: "0.0.0.0:8847", address: "office:8847" };
  value.runtime = { listener: true, error: null };
  mocks.request.mockImplementation((_url, init) => {
    if (init?.method === "POST") {
      value.relations = [relation()];
      return new Promise((resolve) =>
        init.signal.addEventListener("abort", () =>
          resolve({ ok: false, error: { message: "aborted" } }),
        ),
      );
    }
    return Promise.resolve({ ok: true, result: { ...value } });
  });
  await render();
  await act(async () => button("direct.inviteGroup").click());
  await act(async () => button("direct.create").click());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(15000);
  });
  expect(postBodies()).toHaveLength(1);
  expect(host.textContent).toContain("direct.actionUnconfirmed");
  expect(host.textContent).toContain("direct.invitationLost");
});
it("keeps the invitation available when clipboard permission is denied", async () => {
  const text = invitation();
  value.listener = { bind: "0.0.0.0:8847", address: "office:8847" };
  value.runtime = { listener: true, error: null };
  mocks.copy.mockResolvedValue(false);
  mocks.request.mockImplementation(async (_url, init) =>
    init?.method === "POST"
      ? { ok: true, result: { invitation: text } }
      : { ok: true, result: value },
  );
  await render();
  await act(async () => button("direct.inviteGroup").click());
  await act(async () => button("direct.create").click());
  await act(async () => button("direct.copy").click());
  expect(host.textContent).toContain("direct.copyFailed");
  expect(host.querySelector<HTMLTextAreaElement>("textarea[readonly]")?.value).toBe(text);
});

it("retains a saved forwarded address instead of replacing it with local suggestions", async () => {
  value.listener = { bind: "0.0.0.0:8847", address: "gateway.example:14447" };
  await render();
  await act(async () => button("direct.inviteGroup").click());
  expect(host.textContent).toContain("gateway.example:14447");
  expect(host.textContent).toContain("direct.savedAddress");
  expect(host.textContent).not.toContain("192.168.1.10:8847");
  expect(postBodies()).toEqual([]);
});
it("allows choosing an IPv6 interface and pairs it with an IPv6 listener", async () => {
  await render();
  await act(async () => button("direct.inviteGroup").click());
  await act(async () => button("direct.changeAddress").click());
  const address = host.querySelector<HTMLInputElement>("input[placeholder]")!;
  expect(document.activeElement).toBe(address);
  const select = host.querySelector("select")!;
  await act(async () => {
    select.value = "[fd00::2]:8847";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(address.value).toBe("[fd00::2]:8847");
  expect(Array.from(host.querySelectorAll("input")).some((i) => i.value === "[::]:8847")).toBe(
    true,
  );
  expect(postBodies()).toEqual([]);
});
it("explains missing local addresses and never fabricates an endpoint", async () => {
  value.addresses = [];
  await render();
  await act(async () => button("direct.inviteGroup").click());
  expect(host.querySelector<HTMLInputElement>("input[placeholder]")?.value).toBe("");
  expect(host.textContent).toContain("direct.noAddress");
  expect(button("direct.create").disabled).toBe(true);
  await act(async () => button("direct.back").click());
  await act(async () => button("direct.useInvite").click());
  expect(host.querySelector("textarea")).not.toBeNull();
  expect(postBodies()).toEqual([]);
});
it("does not create an invitation after a bind failure, but can retry after recovery", async () => {
  mocks.request.mockImplementation(async (_url, init) => {
    if (init?.method === "POST") {
      const body = JSON.parse(init.body);
      if (body.action === "configure") {
        value.listener = body.listener;
        value.runtime = { listener: false, error: "Address already in use" };
        return { ok: true, result: {} };
      }
      return { ok: true, result: { invitation: invitation() } };
    }
    return { ok: true, result: { ...value } };
  });
  await render();
  await act(async () => button("direct.inviteGroup").click());
  await act(async () => button("direct.create").click());
  expect(host.textContent).toContain("direct.listenerFailed");
  expect(postBodies().map((b) => b.action)).toEqual(["configure"]);
  value.runtime = { listener: true, error: null };
  await act(async () => button("direct.create").click());
  expect(postBodies().map((b) => b.action)).toEqual(["configure", "invite"]);
});
it("stops waiting after a bounded deadline and never resumes from later polling", async () => {
  vi.useFakeTimers();
  value.listener = { bind: "0.0.0.0:8847", address: "office:8847" };
  await render();
  await act(async () => button("direct.inviteGroup").click());
  await act(async () => button("direct.create").click());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10000);
  });
  expect(host.textContent).toContain("direct.prepareTimedOut");
  value.runtime = { listener: true, error: null };
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(postBodies()).toEqual([]);
});
it("aborts preparation on unmount without leaving an automatic invitation behind", async () => {
  vi.useFakeTimers();
  value.listener = { bind: "0.0.0.0:8847", address: "office:8847" };
  await render();
  await act(async () => button("direct.inviteGroup").click());
  await act(async () => button("direct.create").click());
  await act(async () => root.render(null));
  value.runtime = { listener: true, error: null };
  await act(async () => {
    await vi.advanceTimersByTimeAsync(16000);
  });
  expect(postBodies()).toEqual([]);
});
it("rejects receiving settings changed by another session during preparation", async () => {
  value.listener = { bind: "0.0.0.0:8847", address: "office:8847" };
  await render();
  await act(async () => button("direct.inviteGroup").click());
  value.listener = { bind: "0.0.0.0:9922", address: "office:9922" };
  await act(async () => button("direct.create").click());
  expect(host.textContent).toContain("direct.settingsChanged");
  expect(postBodies()).toEqual([]);
});

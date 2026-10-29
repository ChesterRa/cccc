// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { NotificationsTab } from "./NotificationsTab";
import * as push from "../../../features/pushNotifications/subscription";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../../features/pushNotifications/subscription", async (importOriginal) => ({
  // The status table itself is pure; the tab is tested through it.
  PUSH_STATUS_EVENT: (await importOriginal<typeof push>()).PUSH_STATUS_EVENT,
  pushSetupStatus: (await importOriginal<typeof push>()).pushSetupStatus,
  pushSupported: vi.fn(),
  findBrowserSubscription: vi.fn(),
  readPushConfig: vi.fn(),
  isBoundSubscription: vi.fn(),
  existingPushSetup: vi.fn(),
  preparePush: vi.fn(),
  subscribeFromGesture: vi.fn(),
  bindSubscription: vi.fn(),
  removeSubscription: vi.fn(),
  subscriptionKeyMatches: vi.fn(),
}));

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const requestPermission = vi.fn();
const subscription = { endpoint: "https://push.example/sub" } as PushSubscription;
const entry = { subscription } as push.BrowserSubscription;
const prepared = { manager: {} } as push.PreparedPush;
let bound: push.BrowserSubscription | null;

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  vi.stubGlobal("Notification", { permission: "default", requestPermission });
  requestPermission.mockResolvedValue("granted");
  bound = null;
  vi.mocked(push.pushSupported).mockReturnValue(true);
  vi.mocked(push.findBrowserSubscription).mockImplementation(async () => bound);
  vi.mocked(push.readPushConfig).mockResolvedValue({
    vapid_public_key: "fixture",
    subscriptions: [],
    sender: "here",
  });
  vi.mocked(push.isBoundSubscription).mockImplementation(async () => Boolean(bound));
  vi.mocked(push.subscriptionKeyMatches).mockReturnValue(true);
  vi.mocked(push.existingPushSetup).mockResolvedValue(null);
  vi.mocked(push.preparePush).mockResolvedValue(prepared);
  vi.mocked(push.subscribeFromGesture).mockResolvedValue(subscription);
  vi.mocked(push.bindSubscription).mockImplementation(async () => {
    bound = entry;
    vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  });
  vi.mocked(push.removeSubscription).mockImplementation(async () => {
    bound = null;
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<NotificationsTab />));
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const status = () => host.querySelector('[role="status"]')?.textContent;
const alert = () => host.querySelector('[role="alert"]')?.textContent;
function button(label: string): HTMLButtonElement {
  const result = [...host.querySelectorAll("button")].find((item) => item.textContent === label);
  if (!result) throw new Error(`Missing button ${label}`);
  return result;
}

it("requires an explicit gesture for permission and another for subscription", async () => {
  expect(status()).toBe("notifications.status.disabled");
  expect(requestPermission).not.toHaveBeenCalled();
  expect(push.preparePush).not.toHaveBeenCalled();
  await act(async () => {
    button("notifications.enable").click();
    expect(requestPermission).toHaveBeenCalledOnce();
  });
  expect(push.preparePush).toHaveBeenCalledOnce();
  expect(push.subscribeFromGesture).not.toHaveBeenCalled();
  await act(async () => {
    button("notifications.finish").click();
    expect(push.subscribeFromGesture).toHaveBeenCalledExactlyOnceWith(prepared);
  });
  expect(push.bindSubscription).toHaveBeenCalledExactlyOnceWith(subscription);
  expect(status()).toBe("notifications.status.enabled");
  await act(async () => button("notifications.disable").click());
  expect(push.removeSubscription).toHaveBeenCalledExactlyOnceWith(entry);
  expect(status()).toBe("notifications.status.disabled");
});

it("keeps focus on the one button that carries the next step", async () => {
  const next = button("notifications.enable");
  next.focus();
  await act(async () => next.click());
  expect(status()).toBe("notifications.ready");
  expect(button("notifications.finish")).toBe(next);
  expect(document.activeElement).toBe(next);
  await act(async () => next.click());
  expect(status()).toBe("notifications.status.enabled");
  expect(button("notifications.disable")).toBe(next);
  expect(document.activeElement).toBe(next);
});

it("asks for permission again when it was reset after enabling", async () => {
  bound = entry;
  vi.mocked(push.existingPushSetup).mockResolvedValue(prepared);
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.disabled");
  await act(async () => button("notifications.enable").click());
  expect(requestPermission).toHaveBeenCalledOnce();
  expect(push.subscribeFromGesture).not.toHaveBeenCalled();
});

it("keeps the way out of a subscription it made but could not bind", async () => {
  vi.mocked(push.bindSubscription).mockRejectedValue(new Error("push_origin_invalid"));
  vi.mocked(push.findBrowserSubscription).mockImplementation(async () =>
    vi.mocked(push.subscribeFromGesture).mock.calls.length ? entry : null,
  );
  await act(async () => button("notifications.enable").click());
  await act(async () => button("notifications.finish").click());
  // Another tab may have made the subscription the browser handed back, so
  // it is not taken for this page's own.
  expect(status()).toBe("notifications.status.unbound");
  expect(alert()).toBe("notifications.error.originInvalid");
  expect(button("notifications.removeSaved")).toBeDefined();
  // The same step can be tried again.
  expect(button("notifications.finish")).toBeDefined();
});

it("keeps warning about a subscription the browser already had when binding it fails", async () => {
  // subscribe() returns the subscription the browser already has.
  vi.mocked(push.findBrowserSubscription).mockResolvedValue(entry);
  vi.mocked(push.isBoundSubscription).mockResolvedValue(false);
  vi.mocked(push.existingPushSetup).mockResolvedValue(prepared);
  vi.mocked(push.bindSubscription).mockRejectedValue(new Error("push_unavailable"));
  vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.unbound");
  await act(async () => button("notifications.enable").click());
  expect(push.subscribeFromGesture).toHaveBeenCalledOnce();
  expect(status()).toBe("notifications.status.unbound");
  expect(alert()).toBe("notifications.error.serverUnavailable");
  expect(button("notifications.removeSaved")).toBeDefined();
});

it("reports a sign-in that ended while notifications were being prepared", async () => {
  vi.mocked(push.preparePush).mockRejectedValue(new Error("permission_denied"));
  await act(async () => button("notifications.enable").click());
  expect(alert()).toBe("notifications.error.signedOut");
  expect(push.subscribeFromGesture).not.toHaveBeenCalled();
});

it("subscribes in one gesture when permission, key and manager are already available", async () => {
  vi.mocked(push.existingPushSetup).mockResolvedValue(prepared);
  vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  await act(async () => {
    button("notifications.enable").click();
    expect(push.subscribeFromGesture).toHaveBeenCalledExactlyOnceWith(prepared);
  });
  expect(requestPermission).not.toHaveBeenCalled();
  expect(push.preparePush).not.toHaveBeenCalled();
  expect(status()).toBe("notifications.status.enabled");
});

it("does not prepare a key or subscribe when permission is denied", async () => {
  requestPermission.mockResolvedValue("denied");
  const announced = vi.fn();
  window.addEventListener("cccc:push-status", announced);
  await act(async () => button("notifications.enable").click());
  window.removeEventListener("cccc:push-status", announced);
  expect(push.preparePush).not.toHaveBeenCalled();
  expect(push.subscribeFromGesture).not.toHaveBeenCalled();
  expect(status()).toBe("notifications.status.blocked");
  expect(alert()).toBe("notifications.error.permission");
  // The offer on the messages page rereads the status and stops offering.
  expect(announced).toHaveBeenCalled();
});

it("reports blocked permission before enabling without a subscription", async () => {
  vi.stubGlobal("Notification", { permission: "denied", requestPermission });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.blocked");
  expect(host.querySelectorAll("button")).toHaveLength(0);
  expect(requestPermission).not.toHaveBeenCalled();
  vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(status()).toBe("notifications.status.disabled");
  expect(button("notifications.enable")).toBeDefined();
});

it("lets a blocked browser remove an unbound subscription without duplicate controls", async () => {
  bound = entry;
  vi.stubGlobal("Notification", { permission: "denied", requestPermission });
  vi.mocked(push.isBoundSubscription).mockResolvedValue(false);
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.blocked");
  expect(button("notifications.removeSaved")).toBeDefined();
  expect(host.querySelectorAll("button")).toHaveLength(1);
});

it("treats an unreadable server status as unknown and allows a retry", async () => {
  vi.mocked(push.readPushConfig).mockRejectedValueOnce(new Error("offline"));
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.unknown");
  expect(host.querySelector("button")?.textContent).toBe("notifications.retry");
  await act(async () => button("notifications.retry").click());
  expect(status()).toBe("notifications.status.disabled");
});

it("checks the server after a lost binding response before reporting failure", async () => {
  vi.mocked(push.bindSubscription).mockImplementation(async () => {
    bound = entry;
    vi.stubGlobal("Notification", { permission: "granted", requestPermission });
    throw new Error("response lost");
  });
  await act(async () => button("notifications.enable").click());
  await act(async () => button("notifications.finish").click());
  expect(status()).toBe("notifications.status.enabled");
  expect(alert()).toBeUndefined();
});

it("explains an unsupported service and keeps the browser subscription", async () => {
  vi.mocked(push.subscribeFromGesture).mockImplementation(async () => {
    vi.mocked(push.findBrowserSubscription).mockResolvedValue(entry);
    return subscription;
  });
  vi.mocked(push.bindSubscription).mockRejectedValue(new Error("push_service_unsupported"));
  await act(async () => button("notifications.enable").click());
  await act(async () => button("notifications.finish").click());
  // Not connected to this account, with the reason; nothing is removed.
  expect(status()).toBe("notifications.status.unbound");
  expect(alert()).toBe("notifications.error.serviceUnsupported");
  expect(push.removeSubscription).not.toHaveBeenCalled();
});

it("keeps the browser subscription when the sender stops during setup", async () => {
  vi.mocked(push.subscribeFromGesture).mockImplementation(async () => {
    vi.mocked(push.findBrowserSubscription).mockResolvedValue(entry);
    return subscription;
  });
  vi.mocked(push.bindSubscription).mockRejectedValue(new Error("push_sender_unavailable"));
  await act(async () => button("notifications.enable").click());
  vi.mocked(push.readPushConfig).mockResolvedValue({
    vapid_public_key: "fixture",
    subscriptions: [],
    sender: "unavailable",
  });
  await act(async () => button("notifications.finish").click());
  expect(status()).toBe("notifications.status.senderUnavailable");
  expect(alert()).toBe("notifications.error.serverUnavailable");
  // The same step can be tried again once the sender is back.
  expect(button("notifications.finish")).toBeDefined();
  expect(button("notifications.retry")).toBeDefined();
  expect(push.removeSubscription).not.toHaveBeenCalled();
});

it("does not claim to have disabled a subscription when removal fails", async () => {
  bound = entry;
  vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  vi.mocked(push.removeSubscription).mockRejectedValue(new Error("offline"));
  await act(async () => button("notifications.disable").click());
  expect(status()).toBe("notifications.status.enabled");
  expect(alert()).toBe("notifications.error.disable");
});

it("reports a blocked device permission even when the server binding exists", async () => {
  bound = entry;
  vi.stubGlobal("Notification", { permission: "denied", requestPermission });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.blocked");
  expect(button("notifications.disable")).toBeDefined();
});

it.each(["starting", "unavailable"] as const)(
  "reports a %s sender rather than claiming a saved binding can send",
  async (sender) => {
    bound = entry;
    vi.stubGlobal("Notification", { permission: "granted", requestPermission });
    vi.mocked(push.readPushConfig).mockResolvedValue({
      vapid_public_key: "fixture",
      subscriptions: [],
      sender,
    });
    await act(async () => {
      root.unmount();
      root = createRoot(host);
      root.render(<NotificationsTab />);
    });
    expect(status()).toBe("notifications.status.senderUnavailable");
    expect(button("notifications.disable")).toBeDefined();
    expect(button("notifications.retry")).toBeDefined();
    vi.mocked(push.readPushConfig).mockResolvedValue({
      vapid_public_key: "fixture",
      subscriptions: [],
      sender: "elsewhere",
    });
    await act(async () => button("notifications.retry").click());
    expect(status()).toBe("notifications.status.enabled");
  },
);

it("does not offer to enable notifications while the sender is unavailable", async () => {
  vi.mocked(push.readPushConfig).mockResolvedValue({
    vapid_public_key: "fixture",
    subscriptions: [],
    sender: "unavailable",
  });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.senderUnavailable");
  expect([...host.querySelectorAll("button")].map((item) => item.textContent)).toEqual([
    "notifications.retry",
  ]);
});

it("rechecks the account and permission when the window regains focus", async () => {
  bound = entry;
  vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.enabled");
  vi.mocked(push.isBoundSubscription).mockResolvedValue(false);
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(status()).toBe("notifications.status.unbound");
});

it("rechecks the device permission when a background page becomes visible", async () => {
  bound = entry;
  vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.enabled");
  vi.stubGlobal("Notification", { permission: "denied", requestPermission });
  const original = Object.getOwnPropertyDescriptor(document, "visibilityState");
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
  try {
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    expect(status()).toBe("notifications.status.blocked");
  } finally {
    if (original) Object.defineProperty(document, "visibilityState", original);
    else Reflect.deleteProperty(document, "visibilityState");
  }
});

it("does not let an older status check overwrite the account checked on focus", async () => {
  bound = entry;
  vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  let finishOld!: (value: Awaited<ReturnType<typeof push.readPushConfig>>) => void;
  vi.mocked(push.readPushConfig)
    .mockReturnValueOnce(new Promise((resolve) => (finishOld = resolve)))
    .mockResolvedValueOnce({ vapid_public_key: "fixture", subscriptions: [], sender: "here" });
  vi.mocked(push.isBoundSubscription).mockImplementation(async (_, config) =>
    config.subscriptions.includes("old"),
  );
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.checking");
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(status()).toBe("notifications.status.unbound");
  await act(async () =>
    finishOld({ vapid_public_key: "fixture", subscriptions: ["old"], sender: "here" }),
  );
  expect(status()).toBe("notifications.status.unbound");
});

it("rechecks once more if the window refocuses while a status check is in flight", async () => {
  bound = entry;
  vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  let finishFirst!: (value: Awaited<ReturnType<typeof push.readPushConfig>>) => void;
  vi.mocked(push.readPushConfig)
    .mockReturnValueOnce(new Promise((resolve) => (finishFirst = resolve)))
    .mockResolvedValueOnce({
      vapid_public_key: "fixture",
      subscriptions: ["current"],
      sender: "here",
    });
  vi.mocked(push.isBoundSubscription).mockImplementation(async (_, config) =>
    config.subscriptions.includes("current"),
  );
  await act(async () => window.dispatchEvent(new Event("focus")));
  await act(async () => window.dispatchEvent(new Event("focus")));
  await act(async () =>
    finishFirst({ vapid_public_key: "fixture", subscriptions: [], sender: "here" }),
  );
  expect(status()).toBe("notifications.status.enabled");
});

it("does not start a focus check while subscription removal is in flight", async () => {
  bound = entry;
  vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  let finishRemoval!: () => void;
  vi.mocked(push.removeSubscription).mockReturnValueOnce(
    new Promise((resolve) => (finishRemoval = resolve)),
  );
  const previousChecks = vi.mocked(push.readPushConfig).mock.calls.length;
  await act(async () => button("notifications.disable").click());
  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(vi.mocked(push.readPushConfig)).toHaveBeenCalledTimes(previousChecks);
  expect(button("notifications.disable").getAttribute("aria-disabled")).toBe("true");
  bound = null;
  await act(async () => finishRemoval());
  expect(status()).toBe("notifications.status.disabled");
});

it("makes an unbound browser subscription visible and removable after a failed binding", async () => {
  vi.mocked(push.findBrowserSubscription).mockResolvedValue(entry);
  vi.mocked(push.isBoundSubscription).mockResolvedValue(false);
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.unbound");
  expect(host.textContent).toContain("notifications.removeSavedHelp");
  expect(button("notifications.enable")).toBeDefined();
  vi.mocked(push.findBrowserSubscription).mockResolvedValue(null);
  await act(async () => button("notifications.removeSaved").click());
  expect(push.removeSubscription).toHaveBeenCalledExactlyOnceWith(entry);
  expect(status()).toBe("notifications.status.disabled");
});

it("keeps the local recovery action when the server cannot be checked", async () => {
  vi.mocked(push.findBrowserSubscription).mockResolvedValue(entry);
  vi.mocked(push.readPushConfig).mockRejectedValue(new Error("offline"));
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.unknown");
  expect(button("notifications.removeSaved")).toBeDefined();
  expect(button("notifications.retry")).toBeDefined();
});

it("offers to reconnect when the browser key differs from the server key", async () => {
  bound = entry;
  vi.mocked(push.subscriptionKeyMatches).mockReturnValue(false);
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  expect(status()).toBe("notifications.status.keyMismatch");
  expect(button("notifications.removeSaved")).toBeDefined();
  expect(
    [...host.querySelectorAll("button")].some(
      (item) => item.textContent === "notifications.enable",
    ),
  ).toBe(false);
});

it("checks again on focus without dropping the button or announcing a check", async () => {
  bound = entry;
  vi.stubGlobal("Notification", { permission: "granted", requestPermission });
  await act(async () => {
    root.unmount();
    root = createRoot(host);
    root.render(<NotificationsTab />);
  });
  const disable = button("notifications.disable");
  disable.focus();
  let finishCheck!: (value: Awaited<ReturnType<typeof push.readPushConfig>>) => void;
  vi.mocked(push.readPushConfig).mockReturnValueOnce(
    new Promise((resolve) => (finishCheck = resolve)),
  );

  await act(async () => window.dispatchEvent(new Event("focus")));
  // While it checks, the last status and its button stay. The button waits
  // without being disabled, which in browsers would move focus to the page.
  expect(status()).toBe("notifications.status.enabled");
  expect(button("notifications.disable")).toBe(disable);
  expect(disable.disabled).toBe(false);
  expect(disable.getAttribute("aria-disabled")).toBe("true");
  await act(async () => disable.click());
  expect(push.removeSubscription).not.toHaveBeenCalled();

  await act(async () =>
    finishCheck({ vapid_public_key: "fixture", subscriptions: [], sender: "here" }),
  );
  expect(button("notifications.disable")).toBe(disable);
  expect(disable.hasAttribute("aria-disabled")).toBe(false);
  expect(document.activeElement).toBe(disable);
});

it("drops an earlier error once a check gives a definite status", async () => {
  requestPermission.mockResolvedValue("default");
  await act(async () => button("notifications.enable").click());
  expect(alert()).toBe("notifications.error.permission");

  await act(async () => window.dispatchEvent(new Event("focus")));
  expect(alert()).toBeUndefined();
  expect(status()).toBe("notifications.status.disabled");
});

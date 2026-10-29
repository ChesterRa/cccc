// @vitest-environment happy-dom
import { createHash, webcrypto } from "node:crypto";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { apiJson } from "../../services/api/base";
import {
  isBoundSubscription,
  bindSubscription,
  existingPushSetup,
  findBrowserSubscription,
  preparePush,
  pushSetupStatus,
  pushSupported,
  readPushConfig,
  removeSubscription,
  subscribeFromGesture,
  subscriptionKeyMatches,
  unbindBeforeSignOut,
  type BrowserSubscription,
} from "./subscription";

vi.mock("../../services/api/base", () => ({ apiJson: vi.fn() }));
const publicKeyBytes = Buffer.concat([Buffer.from([4]), Buffer.alloc(64, 17)]);
const publicKey = publicKeyBytes.toString("base64url");
const endpoint = "https://push.example/secret-endpoint";
const unsubscribe = vi.fn();
const unregister = vi.fn();
const subscribe = vi.fn();
const getSubscription = vi.fn();
const subscription = {
  endpoint,
  options: { applicationServerKey: Uint8Array.from(publicKeyBytes).buffer },
  toJSON: () => ({ endpoint }),
  unsubscribe,
} as unknown as PushSubscription;
const manager = { subscribe, getSubscription } as unknown as PushManager;
let registration: ServiceWorkerRegistration;

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("crypto", webcrypto);
  unsubscribe.mockResolvedValue(true);
  unregister.mockResolvedValue(true);
  getSubscription.mockResolvedValue(subscription);
  subscribe.mockResolvedValue(subscription);
  registration = {
    scope: new URL("/ui/push/", location.origin).href,
    active: { scriptURL: new URL("/ui/push/sw.js", location.origin).href },
    pushManager: manager,
    unregister,
  } as unknown as ServiceWorkerRegistration;
  Object.defineProperty(navigator, "serviceWorker", {
    configurable: true,
    value: {
      getRegistration: vi.fn().mockResolvedValue(registration),
      register: vi.fn().mockResolvedValue(registration),
    },
  });
  vi.mocked(apiJson).mockResolvedValue({
    ok: true,
    result: { vapid_public_key: publicKey, removed: true },
  });
});
afterEach(() => {
  Reflect.deleteProperty(window, "pushManager");
  Reflect.deleteProperty(navigator, "serviceWorker");
  vi.unstubAllGlobals();
});

describe("notification subscriptions", () => {
  it("reports unsupported when the browser has no Push API", () => {
    vi.stubGlobal("isSecureContext", true);
    vi.stubGlobal("Notification", {});
    vi.stubGlobal("PushManager", class {});
    expect(pushSupported()).toBe(true);
    Reflect.deleteProperty(window, "PushManager");
    expect(pushSupported()).toBe(false);
  });

  it("treats malformed server configuration as unknown instead of a key mismatch", async () => {
    vi.mocked(apiJson).mockResolvedValue({ ok: true, result: { subscriptions: [] } });
    await expect(readPushConfig()).rejects.toThrow("config_unavailable");
    vi.mocked(apiJson).mockResolvedValue({
      ok: true,
      result: { vapid_public_key: publicKey, subscriptions: [], sender: "unexpected" },
    });
    await expect(readPushConfig()).rejects.toThrow("config_unavailable");
    vi.mocked(apiJson).mockResolvedValue({
      ok: true,
      result: { vapid_public_key: publicKey, subscriptions: ["bad hash"] },
    });
    await expect(readPushConfig()).rejects.toThrow("config_unavailable");
    expect(apiJson).toHaveBeenCalledWith("/api/v1/push/config", {
      cache: "no-store",
      signal: expect.any(AbortSignal),
    });
  });

  it("reports an unavailable status when the server never finishes the read", async () => {
    vi.useFakeTimers();
    try {
      vi.mocked(apiJson).mockImplementation(
        async (_path, options) =>
          new Promise((resolve) => {
            options?.signal?.addEventListener("abort", () =>
              resolve({ ok: false, error: { code: "NETWORK_ERROR", message: "timed out" } }),
            );
          }),
      );
      const assertion = expect(readPushConfig()).rejects.toThrow("config_unavailable");
      await vi.advanceTimersByTimeAsync(10000);
      await assertion;
    } finally {
      vi.useRealTimers();
    }
  });

  it("matches the browser endpoint against the current principal's server hashes", async () => {
    const browser = await findBrowserSubscription();
    expect(browser?.subscription).toBe(subscription);
    const hash = createHash("sha256").update(endpoint).digest("hex");
    expect(
      await isBoundSubscription(browser, {
        vapid_public_key: publicKey,
        subscriptions: [hash],
        sender: "here",
      }),
    ).toBe(true);
    expect(
      await isBoundSubscription(browser, {
        vapid_public_key: publicKey,
        subscriptions: ["some-other-owner"],
        sender: "here",
      }),
    ).toBe(false);
    expect(
      await isBoundSubscription(browser, {
        vapid_public_key: null,
        subscriptions: [hash],
        sender: "here",
      }),
    ).toBe(false);
    expect(
      await isBoundSubscription(browser, {
        vapid_public_key: "different",
        subscriptions: [hash],
        sender: "here",
      }),
    ).toBe(false);
  });

  it("sends the browser's application server key when binding", async () => {
    vi.mocked(apiJson).mockResolvedValue({ ok: true, result: { subscribed: true } });
    await bindSubscription(subscription);
    expect(apiJson).toHaveBeenCalledWith("/api/v1/push/subscription", {
      method: "PUT",
      body: JSON.stringify({ endpoint, application_server_key: publicKey }),
      signal: expect.any(AbortSignal),
    });
    expect(subscriptionKeyMatches(subscription, publicKey)).toBe(true);
  });

  it("rejects an unverifiable browser key without binding", async () => {
    const unverifiable = {
      ...subscription,
      options: { applicationServerKey: null },
    } as PushSubscription;
    expect(subscriptionKeyMatches(unverifiable, publicKey)).toBe(false);
    await expect(bindSubscription(unverifiable)).rejects.toThrow("key_unavailable");
    expect(apiJson).not.toHaveBeenCalled();
  });

  it("preserves the server error code when binding fails", async () => {
    vi.mocked(apiJson).mockResolvedValue({
      ok: false,
      error: { code: "push_service_unsupported", message: "unavailable" },
    });
    await expect(bindSubscription(subscription)).rejects.toThrow("push_service_unsupported");
  });

  it("does not use an unrelated worker with a wider scope", async () => {
    Object.defineProperty(navigator, "serviceWorker", {
      configurable: true,
      value: {
        getRegistration: vi
          .fn()
          .mockResolvedValue({ ...registration, scope: new URL("/ui/", location.origin).href }),
      },
    });
    expect(await findBrowserSubscription()).toBeNull();
    expect(getSubscription).not.toHaveBeenCalled();
  });

  it("uses the registered narrow worker directly, without waiting for navigator.serviceWorker.ready", async () => {
    const prepared = await preparePush();
    expect(apiJson).toHaveBeenCalledWith("/api/v1/push/key", {
      method: "POST",
      signal: expect.any(AbortSignal),
    });
    expect(navigator.serviceWorker.register).toHaveBeenCalledWith("/ui/push/sw.js", {
      scope: "/ui/push/",
    });
    expect(prepared.manager).toBe(manager);
    const promise = subscribeFromGesture(prepared);
    expect(subscribe).toHaveBeenCalledOnce();
    await expect(promise).resolves.toBe(subscription);
  });

  it("reports why the server would not hand out its key", async () => {
    vi.mocked(apiJson).mockResolvedValueOnce({
      ok: false,
      error: { code: "permission_denied", message: "the access token is no longer valid" },
    });
    await expect(preparePush()).rejects.toThrow("permission_denied");
    expect(navigator.serviceWorker.register).not.toHaveBeenCalled();
  });

  it("uses the narrow worker even if window.pushManager also exists", async () => {
    Object.defineProperty(window, "pushManager", { configurable: true, value: manager });
    const prepared = await preparePush();
    expect(prepared.manager).toBe(manager);
    expect(navigator.serviceWorker.register).toHaveBeenCalledOnce();
    await subscribeFromGesture(prepared);
    expect(subscribe).toHaveBeenCalledOnce();
  });

  it("uses existing permission, public key and worker without writing or registering", async () => {
    vi.stubGlobal("Notification", { permission: "granted" });
    const ready = await existingPushSetup(
      { vapid_public_key: publicKey, subscriptions: [], sender: "here" },
      null,
    );
    expect(ready?.manager).toBe(manager);
    expect(apiJson).not.toHaveBeenCalled();
    expect(navigator.serviceWorker.register).not.toHaveBeenCalled();
    expect(
      await existingPushSetup({ vapid_public_key: null, subscriptions: [], sender: "here" }, null),
    ).toBeNull();
  });

  it("stops server delivery before unsubscribing or unregistering", async () => {
    const calls: string[] = [];
    vi.mocked(apiJson).mockImplementation(async () => {
      calls.push("server");
      return { ok: true, result: { removed: true } };
    });
    unsubscribe.mockImplementation(async () => {
      calls.push("unsubscribe");
      return true;
    });
    unregister.mockImplementation(async () => {
      calls.push("unregister");
      return true;
    });
    const entry = { subscription, registration } as BrowserSubscription;
    await removeSubscription(entry);
    expect(calls).toEqual(["server", "unsubscribe", "unregister"]);
    expect(apiJson).toHaveBeenCalledWith("/api/v1/push/subscription", {
      method: "DELETE",
      body: JSON.stringify({ endpoint }),
      signal: expect.any(AbortSignal),
    });
  });

  it("still removes the browser subscription when the server cannot be told", async () => {
    vi.mocked(apiJson).mockResolvedValue({
      ok: false,
      error: { code: "NETWORK_ERROR", message: "offline" },
    });
    // The browser's removal stops notifications here; the server forgets the
    // subscription when its next notification to it is refused.
    await expect(removeSubscription({ subscription, registration })).resolves.toBeUndefined();
    expect(unsubscribe).toHaveBeenCalledOnce();
    expect(unregister).toHaveBeenCalledOnce();
  });

  it("asks the server to stop notifying this browser before signing out", async () => {
    vi.stubGlobal("isSecureContext", true);
    vi.stubGlobal("Notification", {});
    vi.stubGlobal("PushManager", class {});
    vi.mocked(apiJson).mockResolvedValue({ ok: true, result: { removed: false } });
    await unbindBeforeSignOut();
    // Only this account's binding goes; the browser keeps its subscription,
    // which another account in this browser profile may hold.
    expect(apiJson).toHaveBeenCalledExactlyOnceWith("/api/v1/push/subscription", {
      method: "DELETE",
      body: JSON.stringify({ endpoint }),
      signal: expect.any(AbortSignal),
    });
    expect(unsubscribe).not.toHaveBeenCalled();
    expect(unregister).not.toHaveBeenCalled();

    // Signing out does not wait on a browser that answers late, and what
    // answers after the wait sends nothing: the page may be someone else's.
    vi.mocked(apiJson).mockClear();
    vi.useFakeTimers();
    try {
      let answer!: (found: ServiceWorkerRegistration) => void;
      vi.mocked(navigator.serviceWorker.getRegistration).mockReturnValue(
        new Promise((settle) => (answer = settle)),
      );
      const done = vi.fn();
      void unbindBeforeSignOut().then(done);
      await vi.advanceTimersByTimeAsync(3000);
      expect(done).toHaveBeenCalledOnce();
      answer(registration);
      await vi.advanceTimersByTimeAsync(0);
      expect(apiJson).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("accepts an already-removed browser subscription but reports one still present", async () => {
    unsubscribe.mockResolvedValue(false);
    getSubscription.mockResolvedValueOnce(null);
    await expect(removeSubscription({ subscription, registration })).resolves.toBeUndefined();
    getSubscription.mockResolvedValueOnce(subscription);
    await expect(removeSubscription({ subscription, registration })).rejects.toThrow(
      "unsubscribe_failed",
    );
  });
});

describe("notification status", () => {
  const reading = {
    permission: "granted" as NotificationPermission,
    sender: "here" as const,
    needsReconnect: false,
    bound: true,
    hasSubscription: true,
  };
  it("reads each state the settings show, in the order they take precedence", () => {
    expect(pushSetupStatus(reading)).toBe("enabled");
    expect(pushSetupStatus({ ...reading, sender: "elsewhere" })).toBe("enabled");
    expect(pushSetupStatus({ ...reading, permission: "denied", sender: "unavailable" })).toBe(
      "blocked",
    );
    expect(pushSetupStatus({ ...reading, sender: "starting", needsReconnect: true })).toBe(
      "senderUnavailable",
    );
    expect(pushSetupStatus({ ...reading, needsReconnect: true, bound: false })).toBe("keyMismatch");
    expect(pushSetupStatus({ ...reading, permission: "default" })).toBe("disabled");
    expect(pushSetupStatus({ ...reading, bound: false })).toBe("unbound");
    expect(pushSetupStatus({ ...reading, bound: false, hasSubscription: false })).toBe("disabled");
  });
});

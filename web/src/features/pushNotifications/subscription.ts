import { apiJson, type ApiResponse } from "../../services/api/base";
import { isPushWorkerScript, PUSH_WORKER_PATH } from "./navigation";

const scopePath = "/ui/push/";

interface PushConfig {
  vapid_public_key: string | null;
  subscriptions: string[];
  sender: "starting" | "here" | "elsewhere" | "unavailable";
}

export interface BrowserSubscription {
  subscription: PushSubscription;
  registration: ServiceWorkerRegistration;
}

export interface PreparedPush {
  manager: PushManager;
  publicKey: Uint8Array<ArrayBuffer>;
}

/** Whether a registration is the dedicated push worker in its own scope. */
export function isPushWorkerRegistration(registration: ServiceWorkerRegistration): boolean {
  const scope = new URL(registration.scope);
  if (scope.origin !== location.origin || scope.pathname !== scopePath) return false;
  return [registration.active, registration.installing, registration.waiting].some(
    (worker) => !!worker && isPushWorkerScript(worker.scriptURL),
  );
}

export function pushSupported(): boolean {
  return Boolean(
    window.isSecureContext &&
    "Notification" in window &&
    "PushManager" in window &&
    "serviceWorker" in navigator,
  );
}

export async function findBrowserSubscription(): Promise<BrowserSubscription | null> {
  const registration = await navigator.serviceWorker.getRegistration(scopePath);
  if (!registration || !isPushWorkerRegistration(registration)) return null;
  const subscription = await registration.pushManager.getSubscription();
  return subscription ? { subscription, registration } : null;
}

/** A notification request gives up after ten seconds rather than leaving the settings busy. */
async function pushRequest<T>(path: string, init: RequestInit): Promise<ApiResponse<T>> {
  const controller = new AbortController();
  const deadline = window.setTimeout(() => controller.abort(), 10000);
  try {
    return await apiJson<T>(path, { ...init, signal: controller.signal });
  } finally {
    window.clearTimeout(deadline);
  }
}

export async function readPushConfig(): Promise<PushConfig> {
  const response = await pushRequest<PushConfig>("/api/v1/push/config", { cache: "no-store" });
  if (
    !response.ok ||
    !response.result ||
    (response.result.vapid_public_key !== null &&
      typeof response.result.vapid_public_key !== "string") ||
    !["starting", "here", "elsewhere", "unavailable"].includes(response.result.sender) ||
    !Array.isArray(response.result.subscriptions) ||
    !response.result.subscriptions.every(
      (hash) => typeof hash === "string" && /^[0-9a-f]{64}$/.test(hash),
    )
  ) {
    throw new Error("config_unavailable");
  }
  if (response.result.vapid_public_key !== null) decodePublicKey(response.result.vapid_public_key);
  return response.result;
}

async function endpointHash(endpoint: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(endpoint));
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function subscriptionPublicKey(subscription: PushSubscription): string | null {
  const key = subscription.options?.applicationServerKey;
  if (!key) return null;
  const bytes = new Uint8Array(key);
  if (bytes.length !== 65 || bytes[0] !== 4) return null;
  const binary = Array.from(bytes, (byte) => String.fromCharCode(byte)).join("");
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function subscriptionKeyMatches(
  subscription: PushSubscription,
  publicKey: string | null,
): boolean {
  const key = subscriptionPublicKey(subscription);
  return key !== null && key === publicKey;
}

export async function isBoundSubscription(
  browser: BrowserSubscription | null,
  config: PushConfig,
): Promise<boolean> {
  return Boolean(
    browser &&
    subscriptionKeyMatches(browser.subscription, config.vapid_public_key) &&
    config.subscriptions.includes(await endpointHash(browser.subscription.endpoint)),
  );
}

/** Reuse an already available manager and key without creating anything. */
export async function existingPushSetup(
  config: PushConfig,
  browser: BrowserSubscription | null,
): Promise<PreparedPush | null> {
  if (Notification.permission !== "granted" || !config.vapid_public_key) return null;
  const publicKey = decodePublicKey(config.vapid_public_key);
  if (browser) {
    return { manager: browser.registration.pushManager, publicKey };
  }
  if ("serviceWorker" in navigator) {
    const registration = await navigator.serviceWorker.getRegistration(scopePath);
    if (registration?.active && isPushWorkerRegistration(registration)) {
      return { manager: registration.pushManager, publicKey };
    }
  }
  return null;
}

function decodePublicKey(value: string): Uint8Array<ArrayBuffer> {
  if (!/^[A-Za-z0-9_-]+$/.test(value)) throw new Error("invalid_public_key");
  const padded = value
    .replace(/-/g, "+")
    .replace(/_/g, "/")
    .padEnd(Math.ceil(value.length / 4) * 4, "=");
  const decoded = atob(padded);
  const key = Uint8Array.from(decoded, (char) => char.charCodeAt(0));
  if (key.length !== 65 || key[0] !== 4) throw new Error("invalid_public_key");
  return key;
}

async function activeWorker(registration: ServiceWorkerRegistration): Promise<void> {
  if (registration.active) return;
  const worker = registration.installing ?? registration.waiting;
  if (!worker) throw new Error("worker_unavailable");
  await new Promise<void>((resolve, reject) => {
    const finish = () => {
      if (registration.active || worker.state === "activated") {
        cleanup();
        resolve();
      } else if (worker.state === "redundant") {
        cleanup();
        reject(new Error("worker_unavailable"));
      }
    };
    const timer = window.setTimeout(() => {
      cleanup();
      reject(new Error("worker_unavailable"));
    }, 15000);
    const cleanup = () => {
      window.clearTimeout(timer);
      worker.removeEventListener("statechange", finish);
    };
    worker.addEventListener("statechange", finish);
    finish();
  });
}

/** Called only after the user has granted notification permission. */
export async function preparePush(): Promise<PreparedPush> {
  const response = await pushRequest<{ vapid_public_key: string }>("/api/v1/push/key", {
    method: "POST",
  });
  if (!response.ok) throw new Error(response.error.code);
  if (typeof response.result?.vapid_public_key !== "string") throw new Error("key_unavailable");
  const publicKey = decodePublicKey(response.result.vapid_public_key);
  const registration = await navigator.serviceWorker.register(PUSH_WORKER_PATH, {
    scope: scopePath,
  });
  await activeWorker(registration);
  if (!isPushWorkerRegistration(registration)) throw new Error("worker_unavailable");
  return { manager: registration.pushManager, publicKey };
}

/** Begin subscribe synchronously inside the click handler, before awaiting anything. */
export function subscribeFromGesture(prepared: PreparedPush): Promise<PushSubscription> {
  return prepared.manager.subscribe({
    userVisibleOnly: true,
    applicationServerKey: prepared.publicKey,
  });
}

export async function bindSubscription(subscription: PushSubscription): Promise<void> {
  const applicationServerKey = subscriptionPublicKey(subscription);
  if (!applicationServerKey) throw new Error("key_unavailable");
  const response = await pushRequest<{ subscribed: boolean }>("/api/v1/push/subscription", {
    method: "PUT",
    body: JSON.stringify({
      ...subscription.toJSON(),
      application_server_key: applicationServerKey,
    }),
  });
  if (!response.ok) throw new Error(response.error.code);
  if (response.result?.subscribed !== true) throw new Error("binding_failed");
}

/**
 * Stop server delivery, then remove the browser subscription. Removing it from
 * the browser is what stops notifications here, so it goes ahead when the
 * server cannot be told: the server forgets the subscription once its next
 * notification to it is refused.
 */
export async function removeSubscription(entry: BrowserSubscription): Promise<void> {
  await pushRequest<{ removed: boolean }>("/api/v1/push/subscription", {
    method: "DELETE",
    body: JSON.stringify({ endpoint: entry.subscription.endpoint }),
  });
  if (!(await entry.subscription.unsubscribe())) {
    const remaining = await entry.registration.pushManager.getSubscription();
    if (remaining?.endpoint === entry.subscription.endpoint) throw new Error("unsubscribe_failed");
  }
  if (isPushWorkerRegistration(entry.registration) && !(await entry.registration.unregister())) {
    throw new Error("unsubscribe_failed");
  }
}

/**
 * Before this browser signs out: while still signed in, ask the server to stop
 * notifying this browser for the account. The server removes only that
 * account's binding, so another account's in this browser profile stays, and
 * the browser keeps its subscription for whoever enables it next. Signing out
 * waits a few seconds at most, and goes ahead whatever happens here.
 */
export async function unbindBeforeSignOut(): Promise<void> {
  let late = false;
  const attempt = (async () => {
    if (!pushSupported()) return;
    const browser = await findBrowserSubscription();
    // Past the wait, the page may be signed in again as someone else.
    if (!browser || late) return;
    await pushRequest<{ removed: boolean }>("/api/v1/push/subscription", {
      method: "DELETE",
      body: JSON.stringify({ endpoint: browser.subscription.endpoint }),
    });
  })().catch(() => undefined);
  let timer = 0;
  await Promise.race([
    attempt,
    new Promise<void>((settle) => {
      timer = window.setTimeout(() => {
        late = true;
        settle();
      }, 3000);
    }),
  ]);
  window.clearTimeout(timer);
}

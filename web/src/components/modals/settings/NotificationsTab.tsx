import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  bindSubscription,
  isBoundSubscription,
  existingPushSetup,
  findBrowserSubscription,
  preparePush,
  PUSH_STATUS_EVENT,
  pushSetupStatus,
  pushSupported,
  readPushConfig,
  removeSubscription,
  subscriptionKeyMatches,
  subscribeFromGesture,
  type BrowserSubscription,
  type PreparedPush,
} from "../../../features/pushNotifications/subscription";
import { SettingsPage } from "./SettingsKit";
import { primaryButtonClass, secondaryButtonClass } from "./types";

type Status =
  | "checking"
  | "enabled"
  | "blocked"
  | "disabled"
  | "unbound"
  | "keyMismatch"
  | "senderUnavailable"
  | "unknown"
  | "unsupported";

const bindingErrors: Record<string, string> = {
  push_unavailable: "serverUnavailable",
  push_sender_unavailable: "serverUnavailable",
  push_service_unsupported: "serviceUnsupported",
  push_subscription_invalid: "subscriptionInvalid",
  push_key_mismatch: "keyMismatch",
  push_origin_invalid: "originInvalid",
  permission_denied: "signedOut",
};

export function NotificationsTab() {
  const { t } = useTranslation("settings");
  const [status, setStatus] = useState<Status>("checking");
  const [bound, setBound] = useState(false);
  const [browser, setBrowser] = useState<BrowserSubscription | null>(null);
  const [prepared, setPrepared] = useState<PreparedPush | null>(null);
  const [available, setAvailable] = useState<PreparedPush | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const mountedRef = useRef(true);
  const refreshVersionRef = useRef(0);
  const focusCheckingRef = useRef(false);
  const focusRecheckRef = useRef(false);

  // Other views, such as the offer on the messages page, follow every status
  // this tab settles on, including a permission the user just denied.
  useEffect(() => {
    if (status !== "checking") window.dispatchEvent(new Event(PUSH_STATUS_EVENT));
  }, [status]);

  const refresh = useCallback(async () => {
    const version = ++refreshVersionRef.current;
    const current = () => mountedRef.current && refreshVersionRef.current === version;
    if (!pushSupported()) {
      if (current()) setStatus("unsupported");
      return false;
    }
    try {
      const local = await findBrowserSubscription();
      if (current()) setBrowser(local);
      const config = await readPushConfig();
      const needsReconnect =
        local && !subscriptionKeyMatches(local.subscription, config.vapid_public_key);
      const matches = !needsReconnect && (await isBoundSubscription(local, config));
      const ready = needsReconnect ? null : await existingPushSetup(config, local);
      if (current()) {
        // A definite answer replaces what an earlier attempt reported.
        setError("");
        setBound(matches);
        setAvailable(ready);
        setStatus(
          pushSetupStatus({
            permission: Notification.permission,
            sender: config.sender,
            needsReconnect: !!needsReconnect,
            bound: matches,
            hasSubscription: !!local,
          }),
        );
      }
      return current() && matches;
    } catch {
      if (current()) {
        setBound(false);
        setAvailable(null);
        setStatus("unknown");
      }
      return false;
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void refresh();
    const onFocus = () => {
      if (busyRef.current) {
        if (focusCheckingRef.current) {
          refreshVersionRef.current += 1;
          focusRecheckRef.current = true;
        }
        return;
      }
      // Checked again quietly: the last status and its buttons stay, so
      // keyboard focus is kept and nothing is announced until it changes.
      busyRef.current = true;
      focusCheckingRef.current = true;
      setBusy(true);
      setPrepared(null);
      void (async () => {
        do {
          focusRecheckRef.current = false;
          await refresh();
        } while (mountedRef.current && focusRecheckRef.current);
      })().finally(() => {
        focusCheckingRef.current = false;
        busyRef.current = false;
        if (mountedRef.current) setBusy(false);
      });
    };
    const onVisibility = () => {
      if (document.visibilityState === "visible") onFocus();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      mountedRef.current = false;
      refreshVersionRef.current += 1;
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [refresh]);

  const start = () => {
    if (busyRef.current) return false;
    busyRef.current = true;
    setBusy(true);
    setError("");
    return true;
  };
  const finish = () => {
    busyRef.current = false;
    if (mountedRef.current) setBusy(false);
  };

  const beginEnable = () => {
    // Without permission granted, ask first even when the worker is ready.
    if (available && Notification.permission === "granted") {
      completeEnable(available);
      return;
    }
    if (!start()) return;
    // This call must run directly inside the user's click, including on iOS.
    let permission: Promise<NotificationPermission>;
    try {
      permission = Notification.requestPermission();
    } catch {
      setError("permission");
      finish();
      return;
    }
    void (async () => {
      try {
        const permissionResult = await permission;
        if (permissionResult !== "granted") {
          if (mountedRef.current) {
            if (permissionResult === "denied") setStatus("blocked");
            setError("permission");
          }
          return;
        }
        const result = await preparePush();
        if (mountedRef.current) setPrepared(result);
      } catch (cause) {
        const code = cause instanceof Error ? cause.message : "";
        if (mountedRef.current) setError(bindingErrors[code] || "prepare");
      } finally {
        finish();
      }
    })();
  };

  const completeEnable = (setup: PreparedPush | null = prepared) => {
    if (!setup || !start()) return;
    // No awaited work before subscribe: some browsers require a fresh gesture.
    let subscription: Promise<PushSubscription>;
    try {
      subscription = subscribeFromGesture(setup);
    } catch {
      setError("subscribe");
      finish();
      return;
    }
    void (async () => {
      try {
        const made = await subscription;
        await bindSubscription(made);
        const confirmed = await refresh();
        if (mountedRef.current) {
          setPrepared(null);
          if (!confirmed) setError("verify");
        }
      } catch (cause) {
        // A lost response may mean the server did save it. Check before retrying;
        // never remove an existing browser subscription on an uncertain result.
        const confirmed = await refresh();
        if (mountedRef.current) {
          if (confirmed) setPrepared(null);
          else {
            const code = cause instanceof Error ? cause.message : "";
            setError(bindingErrors[code] || "subscribe");
          }
        }
      } finally {
        finish();
      }
    })();
  };

  const remove = (entry: BrowserSubscription) => {
    if (!start()) return;
    void (async () => {
      try {
        await removeSubscription(entry);
        await refresh();
        if (mountedRef.current) setPrepared(null);
      } catch {
        await refresh();
        if (mountedRef.current) setError("disable");
      } finally {
        finish();
      }
    })();
  };

  const retry = () => {
    if (!start()) return;
    void refresh().finally(finish);
  };

  // A busy button stays focusable: a disabled one loses keyboard focus to the
  // page in browsers. Its action waits, as start() refuses while busy.
  const waiting = "aria-disabled:cursor-not-allowed aria-disabled:opacity-50";
  // subscribe() hands back whatever subscription this browser profile has,
  // which another tab or account may have made: one not bound to this
  // account is never assumed to be this page's.
  const unbound = browser && !bound;
  const finishing = prepared && status !== "enabled";
  // One button carries the next step, so keyboard focus stays on it as
  // Enable becomes Finish enabling and then Disable.
  const next = finishing
    ? { label: t("notifications.finish"), run: () => completeEnable(), primary: true }
    : status === "disabled" || status === "unbound"
      ? { label: t("notifications.enable"), run: beginEnable, primary: true }
      : (status === "enabled" ||
            ((status === "blocked" || status === "senderUnavailable") && bound)) &&
          browser
        ? { label: t("notifications.disable"), run: () => remove(browser), primary: false }
        : null;

  return (
    <SettingsPage title={t("notifications.title")} description={t("notifications.description")}>
      <section className="space-y-3 border-t border-[var(--glass-border-subtle)] pt-4">
        <p role="status" className="text-sm text-[var(--color-text-secondary)]">
          {finishing && !error ? t("notifications.ready") : t(`notifications.status.${status}`)}
        </p>
        {error && (
          <p role="alert" className="text-sm text-[var(--color-accent-danger)]">
            {t(`notifications.error.${error}`)}
          </p>
        )}
        <div className="flex flex-wrap gap-2">
          {next && (
            <button
              type="button"
              className={`${next.primary ? primaryButtonClass() : secondaryButtonClass()} ${waiting}`}
              aria-disabled={busy || undefined}
              onClick={next.run}
            >
              {next.label}
            </button>
          )}
          {unbound && status !== "checking" && (
            <button
              type="button"
              className={`${secondaryButtonClass()} ${waiting}`}
              aria-disabled={busy || undefined}
              onClick={() => remove(browser)}
            >
              {t("notifications.removeSaved")}
            </button>
          )}
          {(status === "unknown" || status === "senderUnavailable") && (
            <button
              type="button"
              className={`${secondaryButtonClass()} ${waiting}`}
              aria-disabled={busy || undefined}
              onClick={retry}
            >
              {t("notifications.retry")}
            </button>
          )}
        </div>
        {unbound && status !== "checking" && (
          <p className="text-sm text-[var(--color-text-secondary)]">
            {t("notifications.removeSavedHelp")}
          </p>
        )}
      </section>
    </SettingsPage>
  );
}

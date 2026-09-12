import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";
import type { MembershipState } from "../../../types";
import {
  membershipApprovalUrl,
  membershipOwnsReach,
  membershipReachStatus,
} from "./reachMembershipModel";

function pollDelayMs(membership: MembershipState | null): number {
  const configured = Number(membership?.pending?.interval ?? 5);
  const seconds = Number.isFinite(configured) ? Math.max(1, configured) : 5;
  return seconds * 1000;
}

function openPendingWindow(): Window | null {
  const popup = window.open("about:blank", "_blank");
  if (popup) {
    popup.opener = null;
  }
  return popup;
}

function finishPendingWindow(
  popup: Window | null,
  membership: MembershipState,
  language: string,
): void {
  const approvalUrl = membershipApprovalUrl(membership, language);
  if (!approvalUrl) {
    popup?.close();
    return;
  }
  if (popup) {
    popup.location.replace(approvalUrl);
    return;
  }
  window.open(approvalUrl, "_blank", "noopener,noreferrer");
}

export interface MembershipController {
  membership: MembershipState | null;
  membershipBusy: boolean;
  membershipError: string;
  membershipPollReady: boolean;
  reachBusy: boolean;
  reachAction: "starting" | "stopping" | null;
  reachChecking: boolean;
  reachCheckExpired: boolean;
  checkReach: () => void;
  refresh: () => Promise<MembershipState | null>;
  connect: () => Promise<boolean>;
  poll: () => Promise<boolean>;
  disconnect: () => Promise<boolean>;
  startReach: () => Promise<boolean>;
  stopReach: () => Promise<boolean>;
}

export function useMembershipController(active = true): MembershipController {
  const { t, i18n } = useTranslation("settings");
  const [membership, setMembership] = useState<MembershipState | null>(null);
  const [membershipBusy, setMembershipBusy] = useState(active);
  const [membershipError, setMembershipError] = useState("");
  const [reachBusy, setReachBusy] = useState(false);
  const [reachAction, setReachAction] = useState<"starting" | "stopping" | null>(null);
  const [checksRemaining, setChecksRemaining] = useState(0);
  const [reachCheckExpired, setReachCheckExpired] = useState(false);
  const checkDeadline = useRef(0);
  const checkGeneration = useRef(0);
  const requestGeneration = useRef(0);
  const statusRequest = useRef<AbortController | null>(null);
  const [pollNotBefore, setPollNotBefore] = useState(0);
  const [, setClock] = useState(0);
  const pollNotBeforeRef = useRef(0);
  const pollFailureCountRef = useRef(0);

  const cancelReachCheck = useCallback(() => {
    checkGeneration.current += 1;
    setChecksRemaining(0);
    setReachCheckExpired(false);
  }, []);

  const applyMembership = useCallback(
    (next: MembershipState | null) => {
      pollFailureCountRef.current = 0;
      const nextPollAt = !next?.logged_in && next?.pending ? Date.now() + pollDelayMs(next) : 0;
      pollNotBeforeRef.current = nextPollAt;
      setPollNotBefore(nextPollAt);
      setMembership(next);
      // Manual refresh and automatic checks must settle the same state. Also
      // invalidate pending check callbacks so they cannot restore an old warning.
      if (
        next &&
        (!next.logged_in ||
          next.cut ||
          next.disabled ||
          membershipReachStatus(next) === "online" ||
          next.cloudflared?.running === false ||
          !membershipOwnsReach(next))
      ) {
        cancelReachCheck();
      }
    },
    [cancelReachCheck],
  );

  const deferPollAfterFailure = useCallback(() => {
    const failureCount = Math.min(pollFailureCountRef.current + 1, 4);
    pollFailureCountRef.current = failureCount;
    const delay = Math.min(60_000, pollDelayMs(membership) * 2 ** failureCount);
    const nextPollAt = Date.now() + delay;
    pollNotBeforeRef.current = nextPollAt;
    setPollNotBefore(nextPollAt);
  }, [membership]);

  const refresh = useCallback(
    async (timeoutMs = 5_000): Promise<MembershipState | null> => {
      const generation = ++requestGeneration.current;
      statusRequest.current?.abort();
      const controller = new AbortController();
      statusRequest.current = controller;
      const timeout = window.setTimeout(() => controller.abort(), timeoutMs);
      setMembershipBusy(true);
      try {
        const response = await api.fetchMembership(controller.signal);
        if (generation !== requestGeneration.current) return null;
        if (!response.ok || !response.result?.membership) {
          setMembershipError(response.error?.message || t("webAccess.reach.loadFailed"));
          return null;
        }
        applyMembership(response.result.membership);
        setMembershipError("");
        return response.result.membership;
      } catch {
        if (generation !== requestGeneration.current) return null;
        setMembershipError(t("webAccess.reach.loadFailed"));
        return null;
      } finally {
        window.clearTimeout(timeout);
        if (generation === requestGeneration.current) setMembershipBusy(false);
      }
    },
    [applyMembership, t],
  );

  useEffect(() => {
    if (!active) return;
    void refresh();
    return () => {
      requestGeneration.current += 1;
      statusRequest.current?.abort();
    };
  }, [active, refresh]);

  const checkReach = useCallback(() => {
    checkGeneration.current += 1;
    checkDeadline.current = Date.now() + 45_000;
    setReachCheckExpired(false);
    setChecksRemaining(6);
  }, []);

  useEffect(() => {
    if (!active || !checksRemaining || membershipBusy || reachBusy) return;
    const generation = checkGeneration.current;
    let timer: number | undefined;
    const schedule = () => {
      window.clearTimeout(timer);
      if (document.hidden) return;
      timer = window.setTimeout(
        async () => {
          if (Date.now() >= checkDeadline.current) {
            setChecksRemaining(0);
            setReachCheckExpired(true);
            return;
          }
          const next = await refresh(Math.min(5_000, checkDeadline.current - Date.now()));
          if (generation !== checkGeneration.current) return;
          // A failed local request may still be queued in the daemon. Do not
          // pile up more requests; leave an explicit, manually retryable result.
          if (!next) {
            setChecksRemaining(0);
            setReachCheckExpired(true);
            return;
          }
          if (checksRemaining === 1 || Date.now() >= checkDeadline.current) {
            setChecksRemaining(0);
            setReachCheckExpired(true);
          } else {
            setChecksRemaining(checksRemaining - 1);
          }
        },
        checksRemaining === 6 ? 0 : 5_000,
      );
    };
    schedule();
    document.addEventListener("visibilitychange", schedule);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", schedule);
    };
  }, [active, checksRemaining, membershipBusy, reachBusy, refresh]);

  const connect = useCallback(async (): Promise<boolean> => {
    if (membershipBusy) return false;
    cancelReachCheck();
    requestGeneration.current += 1;
    const popup = openPendingWindow();
    setMembershipBusy(true);
    setMembershipError("");
    try {
      if (membership?.logged_in || membership?.cut || membership?.disabled) {
        const retired = await api.logoutMembership();
        if (!retired.ok || !retired.result?.membership) {
          setMembershipError(retired.error?.message || t("webAccess.reach.disconnectFailed"));
          popup?.close();
          return false;
        }
        applyMembership(retired.result.membership);
      }
      const response = await api.startMembershipLogin();
      if (!response.ok || !response.result?.membership) {
        setMembershipError(response.error?.message || t("webAccess.reach.connectFailed"));
        popup?.close();
        return false;
      }
      applyMembership(response.result.membership);
      finishPendingWindow(
        popup,
        response.result.membership,
        i18n.resolvedLanguage || i18n.language,
      );
      return true;
    } catch {
      setMembershipError(t("webAccess.reach.connectFailed"));
      popup?.close();
      return false;
    } finally {
      setMembershipBusy(false);
    }
  }, [
    applyMembership,
    cancelReachCheck,
    i18n.language,
    i18n.resolvedLanguage,
    membership,
    membershipBusy,
    t,
  ]);

  const poll = useCallback(async (): Promise<boolean> => {
    const now = Date.now();
    if (membershipBusy || now < pollNotBeforeRef.current) return false;
    const reservedUntil = now + pollDelayMs(membership);
    pollNotBeforeRef.current = reservedUntil;
    setPollNotBefore(reservedUntil);
    setMembershipBusy(true);
    setMembershipError("");
    try {
      const response = await api.pollMembershipLogin();
      if (!response.ok || !response.result?.membership) {
        setMembershipError(response.error?.message || t("webAccess.reach.pollFailed"));
        deferPollAfterFailure();
        return false;
      }
      applyMembership(response.result.membership);
      return true;
    } catch {
      setMembershipError(t("webAccess.reach.pollFailed"));
      deferPollAfterFailure();
      return false;
    } finally {
      setMembershipBusy(false);
    }
  }, [applyMembership, deferPollAfterFailure, membership, membershipBusy, t]);

  const pendingCode = String(membership?.pending?.user_code || "").trim();
  const membershipPollReady = Boolean(pendingCode) && Date.now() >= pollNotBefore;

  useEffect(() => {
    if (!active || membership?.logged_in || !pendingCode || membershipBusy) return;
    const delay = Math.max(0, pollNotBefore - Date.now()) + 25;
    const timer = window.setTimeout(() => {
      setClock((value) => value + 1);
      void poll();
    }, delay);
    return () => window.clearTimeout(timer);
  }, [active, membership?.logged_in, membershipBusy, pendingCode, poll, pollNotBefore]);

  const disconnect = useCallback(async (): Promise<boolean> => {
    if (membershipBusy) return false;
    cancelReachCheck();
    requestGeneration.current += 1;
    setMembershipBusy(true);
    setMembershipError("");
    try {
      const response = await api.logoutMembership();
      if (!response.ok || !response.result?.membership) {
        setMembershipError(response.error?.message || t("webAccess.reach.disconnectFailed"));
        return false;
      }
      applyMembership(response.result.membership);
      return true;
    } catch {
      setMembershipError(t("webAccess.reach.disconnectFailed"));
      return false;
    } finally {
      setMembershipBusy(false);
    }
  }, [applyMembership, cancelReachCheck, membershipBusy, t]);

  const startReach = useCallback(async (): Promise<boolean> => {
    if (reachBusy || membershipBusy) return false;
    cancelReachCheck();
    requestGeneration.current += 1;
    setReachBusy(true);
    setReachAction("starting");
    setMembershipError("");
    try {
      const response = await api.startMembershipReach();
      if (!response.ok || !response.result?.membership) {
        setMembershipError(response.error?.message || t("webAccess.reach.startFailed"));
        return false;
      }
      applyMembership(response.result.membership);
      checkReach();
      return true;
    } catch {
      setMembershipError(t("webAccess.reach.startFailed"));
      return false;
    } finally {
      setReachBusy(false);
      setReachAction(null);
    }
  }, [applyMembership, cancelReachCheck, checkReach, membershipBusy, reachBusy, t]);

  const stopReach = useCallback(async (): Promise<boolean> => {
    if (reachBusy || membershipBusy) return false;
    cancelReachCheck();
    requestGeneration.current += 1;
    setReachBusy(true);
    setReachAction("stopping");
    setMembershipError("");
    try {
      const response = await api.stopMembershipReach();
      if (!response.ok || !response.result?.membership) {
        setMembershipError(response.error?.message || t("webAccess.reach.stopFailed"));
        return false;
      }
      applyMembership(response.result.membership);
      return true;
    } catch {
      setMembershipError(t("webAccess.reach.stopFailed"));
      return false;
    } finally {
      setReachBusy(false);
      setReachAction(null);
    }
  }, [applyMembership, cancelReachCheck, membershipBusy, reachBusy, t]);

  return {
    membership,
    membershipBusy,
    membershipError,
    membershipPollReady,
    reachBusy,
    reachAction,
    reachChecking: checksRemaining > 0,
    reachCheckExpired,
    checkReach,
    refresh,
    connect,
    poll,
    disconnect,
    startReach,
    stopReach,
  };
}

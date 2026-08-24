import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";
import type { MembershipState } from "../../../types";
import { membershipApprovalUrl } from "./reachMembershipModel";

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
  const [pollNotBefore, setPollNotBefore] = useState(0);
  const [, setClock] = useState(0);
  const pollNotBeforeRef = useRef(0);
  const pollFailureCountRef = useRef(0);

  const applyMembership = useCallback((next: MembershipState | null) => {
    pollFailureCountRef.current = 0;
    const nextPollAt = !next?.logged_in && next?.pending ? Date.now() + pollDelayMs(next) : 0;
    pollNotBeforeRef.current = nextPollAt;
    setPollNotBefore(nextPollAt);
    setMembership(next);
  }, []);

  const deferPollAfterFailure = useCallback(() => {
    const failureCount = Math.min(pollFailureCountRef.current + 1, 4);
    pollFailureCountRef.current = failureCount;
    const delay = Math.min(60_000, pollDelayMs(membership) * 2 ** failureCount);
    const nextPollAt = Date.now() + delay;
    pollNotBeforeRef.current = nextPollAt;
    setPollNotBefore(nextPollAt);
  }, [membership]);

  const refresh = useCallback(async (): Promise<MembershipState | null> => {
    setMembershipBusy(true);
    try {
      const response = await api.fetchMembership();
      if (!response.ok || !response.result?.membership) {
        setMembershipError(response.error?.message || t("webAccess.reach.loadFailed"));
        return null;
      }
      applyMembership(response.result.membership);
      setMembershipError("");
      return response.result.membership;
    } catch {
      setMembershipError(t("webAccess.reach.loadFailed"));
      return null;
    } finally {
      setMembershipBusy(false);
    }
  }, [applyMembership, t]);

  useEffect(() => {
    if (!active) return;
    void refresh();
  }, [active, refresh]);

  const connect = useCallback(async (): Promise<boolean> => {
    if (membershipBusy) return false;
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
  }, [applyMembership, i18n.language, i18n.resolvedLanguage, membership, membershipBusy, t]);

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
  }, [applyMembership, membershipBusy, t]);

  const startReach = useCallback(async (): Promise<boolean> => {
    if (reachBusy) return false;
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
      return true;
    } catch {
      setMembershipError(t("webAccess.reach.startFailed"));
      return false;
    } finally {
      setReachBusy(false);
      setReachAction(null);
    }
  }, [applyMembership, reachBusy, t]);

  const stopReach = useCallback(async (): Promise<boolean> => {
    if (reachBusy) return false;
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
  }, [applyMembership, reachBusy, t]);

  return {
    membership,
    membershipBusy,
    membershipError,
    membershipPollReady,
    reachBusy,
    reachAction,
    refresh,
    connect,
    poll,
    disconnect,
    startReach,
    stopReach,
  };
}

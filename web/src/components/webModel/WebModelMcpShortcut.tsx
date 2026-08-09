import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../services/api";
import { classNames } from "../../utils/classNames";
import { copyTextToClipboard } from "../../utils/copy";
import {
  findActiveWebModelConnector,
  webModelConnectorMcpUrl,
} from "../../utils/webModelConnector";
import { CopyIcon, SettingsIcon } from "../Icons";

interface WebModelMcpShortcutProps {
  groupId: string;
  actorId: string;
  actorRunning: boolean;
  isVisible: boolean;
  readOnly?: boolean;
  onOpenSettings: () => void;
}

type Feedback = "idle" | "copied" | "failed";

export function WebModelMcpShortcut({
  groupId,
  actorId,
  actorRunning,
  isVisible,
  readOnly,
  onOpenSettings,
}: WebModelMcpShortcutProps) {
  const { t } = useTranslation("chat");
  const [connector, setConnector] = useState<api.WebModelConnector | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>("idle");
  const enabled = Boolean(isVisible && !readOnly && groupId && actorId);

  const fetchConnector = useCallback(async () => {
    const response = await api.fetchWebModelConnectors();
    if (!response.ok) throw new Error(response.error?.message || "connector lookup failed");
    return findActiveWebModelConnector(response.result?.connectors || [], groupId, actorId);
  }, [actorId, groupId]);

  useEffect(() => {
    let cancelled = false;
    setFeedback("idle");
    if (!enabled) {
      setConnector(null);
      setLoaded(false);
      return () => {
        cancelled = true;
      };
    }
    setLoaded(false);
    void fetchConnector()
      .then((next) => {
        if (!cancelled) setConnector(next);
      })
      .catch(() => {
        if (!cancelled) setConnector(null);
      })
      .finally(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, [enabled, fetchConnector]);

  if (readOnly) return null;

  const mcpUrl = webModelConnectorMcpUrl(connector);
  const label = !actorRunning
    ? t("webModelDelivery.mcpStartFirst")
    : busy || !loaded
      ? t("webModelDelivery.mcpLoading")
      : feedback === "copied"
        ? t("webModelDelivery.mcpCopied")
        : feedback === "failed"
          ? t("webModelDelivery.mcpRetry")
          : mcpUrl
            ? t("webModelDelivery.copyMcpUrl")
            : t("webModelDelivery.setupMcp");
  const title = !actorRunning
    ? t("webModelDelivery.mcpStartFirstHint")
    : feedback === "failed"
      ? t("webModelDelivery.mcpCopyFailed")
      : label;

  const handleClick = async () => {
    if (!actorRunning || busy || !enabled) return;
    setBusy(true);
    setFeedback("idle");
    try {
      const current = await fetchConnector();
      setConnector(current);
      setLoaded(true);
      const currentUrl = webModelConnectorMcpUrl(current);
      if (!currentUrl) {
        onOpenSettings();
        return;
      }
      setFeedback((await copyTextToClipboard(currentUrl)) ? "copied" : "failed");
    } catch {
      setFeedback("failed");
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      type="button"
      onClick={() => void handleClick()}
      disabled={!actorRunning || busy || !loaded || !enabled}
      className={classNames(
        "inline-flex h-10 shrink-0 items-center justify-center gap-1.5 rounded-xl border px-3 text-xs font-semibold transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/35 disabled:cursor-not-allowed disabled:opacity-50",
        "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
      )}
      title={title}
      aria-label={title}
    >
      {mcpUrl ? (
        <CopyIcon size={15} aria-hidden="true" />
      ) : (
        <SettingsIcon size={15} aria-hidden="true" />
      )}
      <span className="hidden whitespace-nowrap lg:inline">{label}</span>
    </button>
  );
}

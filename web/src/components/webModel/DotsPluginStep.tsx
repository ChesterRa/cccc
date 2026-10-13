import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import { copyTextToClipboard } from "../../utils/copy";
import { ChatGPTPluginSetup } from "../modals/settings/ChatGPTPluginSetup";
import { connectorAddress } from "../modals/settings/connectorAddress";
import { Button } from "../ui/button";

type Props = {
  /** Read only while the step is on screen for an administrator. */
  isVisible: boolean;
  disabled: boolean;
  /** The way to the shared ChatGPT settings, which stays reachable. */
  sharedSettings: ReactNode;
  /**
   * Reports a creation in flight, so a host dialog keeps itself open: the URL
   * it returns is shown once and would be lost with the dialog.
   */
  onBusyChange?: (busy: boolean) => void;
};

type Shared = { exists: boolean; base?: string };

/**
 * Step 1 of Dots setup. When the shared ChatGPT connector does not exist yet,
 * create it here in one click and hand over its private URL at once: it is
 * only ever shown on creation, so it cannot be fetched later.
 */
export function DotsPluginStep({ isVisible, disabled, sharedSettings, onBusyChange }: Props) {
  const { t } = useTranslation("settings");
  const [shared, setShared] = useState<Shared | null>(null);
  const [url, setUrl] = useState("");
  const [copied, setCopied] = useState(false);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState("");
  const [readFailed, setReadFailed] = useState(false);
  const [refresh, setRefresh] = useState(0);

  useEffect(() => {
    if (!isVisible) return;
    let cancelled = false;
    setShared(null);
    setReadFailed(false);
    void api
      .fetchWebModelConnectors()
      .then((listed) => {
        if (cancelled) return;
        if (!listed.ok) {
          setReadFailed(true);
          return;
        }
        setShared({
          exists: listed.result.connectors.some(
            (item) => !item.revoked && (item.provider || "chatgpt_web") === "chatgpt_web",
          ),
          base: listed.result.connector_base_url,
        });
      })
      .catch(() => {
        if (!cancelled) setReadFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [isVisible, refresh]);

  const create = async () => {
    if (creating || disabled) return;
    setCreating(true);
    onBusyChange?.(true);
    setError("");
    try {
      // Never rotate: this page may be stale and a connector may exist by now.
      const created = await api.createWebModelConnector("chatgpt_web", { createOnly: true });
      if (!created.ok) {
        if (created.error.code === "connector_exists") {
          setShared((previous) => ({ ...previous, exists: true }));
          setError(t("dotsActor.setup.connectorExists"));
        } else setError(created.error.message);
        return;
      }
      const token = created.result.connector.connector_url_path_token || "";
      setUrl(token ? new URL(token, window.location.href).toString() : "");
      setShared((previous) => ({ ...previous, exists: true }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setCreating(false);
      onBusyChange?.(false);
    }
  };

  const address = connectorAddress(shared?.base ?? "");
  const needsConnector = shared !== null && !shared.exists;
  return (
    <div className="flex flex-col gap-3" data-dots-plugin-step>
      {url ? (
        <ChatGPTPluginSetup
          disabled={disabled}
          urlCopied={copied}
          onCopyUrl={() =>
            void copyTextToClipboard(url).then((ok) =>
              ok ? setCopied(true) : setError(t("webModelShared.copyFailed")),
            )
          }
          onCopyFailed={() => setError(t("webModelShared.copyFailed"))}
        />
      ) : needsConnector ? (
        address.kind === "https" ? (
          <p className="text-xs leading-relaxed text-[var(--color-text-secondary)]">
            {t("dotsActor.setup.noConnector")}
          </p>
        ) : (
          <p className="text-xs leading-relaxed text-amber-700 dark:text-amber-300">
            {address.kind === "missing"
              ? t("webModelShared.publicUrlMissing")
              : t("webModelShared.publicUrlInvalid", { url: address.origin })}
          </p>
        )
      ) : null}
      {readFailed ? (
        <div className="flex flex-wrap items-center gap-2">
          <p className="text-xs text-rose-600 dark:text-rose-400" role="alert">
            {t("dotsActor.setup.connectorReadFailed")}
          </p>
          <Button
            type="button"
            variant="outline"
            disabled={creating}
            onClick={() => setRefresh((value) => value + 1)}
          >
            {t("common:retry")}
          </Button>
        </div>
      ) : null}
      {error ? (
        <p className="text-xs text-rose-600 dark:text-rose-400" role="alert">
          {error}
        </p>
      ) : null}
      <div className="flex flex-wrap gap-2">
        {needsConnector && !url && address.kind === "https" ? (
          <Button type="button" disabled={disabled || creating} onClick={() => void create()}>
            {t(creating ? "dotsActor.setup.creatingConnector" : "dotsActor.setup.createConnector")}
          </Button>
        ) : null}
        {sharedSettings}
      </div>
    </div>
  );
}

import { useState } from "react";
import { useTranslation } from "react-i18next";
import { copyTextToClipboard } from "../../../utils/copy";
import { primaryButtonClass, secondaryButtonClass } from "./types";

const CHATGPT_PLUGINS_URL = "https://chatgpt.com/settings/plugins-settings";
const PLUGIN_NAME = "CCCC";

interface Props {
  disabled: boolean;
  urlCopied: boolean;
  onCopyUrl: () => void;
  onCopyFailed: () => void;
}

/**
 * The one manual step a Dot cannot do itself: adding CCCC as a custom MCP
 * plugin in the ChatGPT account. Every value to enter is one click away.
 */
export function ChatGPTPluginSetup({ disabled, urlCopied, onCopyUrl, onCopyFailed }: Props) {
  const { t } = useTranslation("settings");
  const [nameCopied, setNameCopied] = useState(false);
  const fields = [
    {
      key: "name",
      value: <code className="font-mono">{PLUGIN_NAME}</code>,
      action: (
        <button
          type="button"
          className={secondaryButtonClass("sm")}
          disabled={disabled}
          onClick={() =>
            void copyTextToClipboard(PLUGIN_NAME).then((ok) =>
              ok ? setNameCopied(true) : onCopyFailed(),
            )
          }
        >
          {t(nameCopied ? "webModelShared.copied" : "webModelShared.pluginSetup.copyName")}
        </button>
      ),
    },
    {
      key: "url",
      // The secret stays off screen; copying is the only way it leaves.
      value: (
        <span className="text-[var(--color-text-muted)]">
          {t("webModelShared.pluginSetup.urlHidden")}
        </span>
      ),
      action: (
        <button
          type="button"
          className={primaryButtonClass()}
          disabled={disabled}
          onClick={onCopyUrl}
        >
          {t(urlCopied ? "webModelShared.copied" : "webModelShared.copy")}
        </button>
      ),
    },
    { key: "auth", value: <code className="font-mono">No authentication</code>, action: null },
  ];
  return (
    <div className="space-y-3" data-chatgpt-plugin-setup>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-[13px] font-medium text-[var(--color-text-primary)]">
          {t("webModelShared.pluginSetup.title")}
        </p>
        <a
          href={CHATGPT_PLUGINS_URL}
          target="_blank"
          rel="noopener noreferrer"
          className={secondaryButtonClass("sm")}
        >
          {t("webModelShared.pluginSetup.openPlugins")}
        </a>
      </div>
      <p className="text-xs leading-5 text-[var(--color-text-muted)]">
        {t("webModelShared.pluginSetup.path")}
      </p>
      <dl className="@container/plugin-fields divide-y divide-[var(--glass-border-subtle)] rounded-[10px] border border-[var(--glass-border-subtle)]">
        {fields.map((field) => (
          <div
            key={field.key}
            data-plugin-field={field.key}
            className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1 px-3.5 py-2.5 text-[13px] @lg/plugin-fields:grid-cols-[6rem_minmax(0,1fr)_auto]"
          >
            <dt className="col-start-1 row-start-1 text-[var(--color-text-secondary)]">
              {t(`webModelShared.pluginSetup.${field.key}`)}
            </dt>
            <dd className="col-span-2 col-start-1 row-start-2 min-w-0 break-words @lg/plugin-fields:col-span-1 @lg/plugin-fields:col-start-2 @lg/plugin-fields:row-start-1">
              {field.value}
            </dd>
            {field.action ? (
              <dd className="col-start-2 row-start-1 @lg/plugin-fields:col-start-3">
                {field.action}
              </dd>
            ) : null}
          </div>
        ))}
      </dl>
    </div>
  );
}

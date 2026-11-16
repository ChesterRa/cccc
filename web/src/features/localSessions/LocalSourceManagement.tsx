import { useTranslation } from "react-i18next";
import { LocalSourceConfirmation } from "./LocalSourceConfirmation";
import { LocalSourceDiagnostic } from "./LocalSourceDiagnostic";
import { LocalSourceActions, type SourceDirectory, type SourceManager } from "./LocalSourceActions";

export function LocalSourceManagement({
  directory,
  management,
  busy,
}: {
  directory: SourceDirectory;
  management: SourceManager;
  busy: boolean;
}) {
  const { t } = useTranslation("layout");
  const missing = (["source_control", "source_diagnostics"] as const).filter(
    (capability) => directory.capabilities?.[capability] !== true,
  );
  return (
    <>
      {!directory.loading && !directory.error && (
        <p role="status" className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
          {missing.length
            ? t("sourceControls.upgrade", { capabilities: missing.join(", ") })
            : t("sourceControls.requirements")}
        </p>
      )}
      {!management.target && management.error && (
        <p role="alert" className="break-words text-sm text-[var(--color-danger)]">
          {management.error}
        </p>
      )}
      {!management.target && management.notice && (
        <p role="status" className="break-words text-sm">
          {management.notice}
        </p>
      )}
      {!!directory.removed_sources?.length && (
        <details className="min-w-0 border border-[var(--glass-border)] p-4">
          <summary className="min-h-11 cursor-pointer text-sm font-semibold">
            {t("sourceControls.removedList")} · {directory.removed_sources.length}
          </summary>
          <ul className="divide-y divide-[var(--glass-border)]">
            {directory.removed_sources.map((source) => (
              <li key={source.source_id} className="min-w-0 space-y-3 py-4">
                <h4 className="break-words text-sm font-semibold [overflow-wrap:anywhere]">
                  {source.title}
                </h4>
                <LocalSourceDiagnostic
                  state={directory.source_states?.[source.source_id]}
                  supported={!!directory.capabilities?.source_diagnostics}
                />
                <LocalSourceActions
                  source={source}
                  directory={directory}
                  management={management}
                  busy={busy}
                />
              </li>
            ))}
          </ul>
        </details>
      )}
      <LocalSourceConfirmation management={management} />
    </>
  );
}

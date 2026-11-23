import { useTranslation } from "react-i18next";
import type { useSessionNodes } from "./useSessionNodes";

type Status = Exclude<ReturnType<typeof useSessionNodes>["directory"]["status"], "ready">;

export function SessionNodeDirectoryNotice({ status }: { status: Status }) {
  const { t } = useTranslation("layout");
  if (status === "unsupported")
    return (
      <div role="status" className="space-y-1">
        <h3 className="text-base font-semibold">{t("sessionNodes.backendUpgradeTitle")}</h3>
        <p className="text-sm text-[var(--color-text-secondary)]">
          {t("sessionNodes.upgradeRequired")}
        </p>
      </div>
    );
  if (status === "loading" || status === "rechecking")
    return <p role="status">{t("sessionNodes.checkingBackend")}</p>;
  return (
    <p role="alert">
      {t(status === "forbidden" ? "sessionNodes.adminRequired" : "sessionNodes.directoryFailed")}
    </p>
  );
}

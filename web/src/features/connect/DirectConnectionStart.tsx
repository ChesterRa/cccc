import { Cable } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";

export function DirectConnectionStart({
  empty,
  disabled,
  onStart,
}: {
  empty: boolean;
  disabled: boolean;
  onStart: (task: "invite" | "join") => void;
}) {
  const { t } = useTranslation("layout");
  const actions = (
    <div className={empty ? "grid w-full max-w-md gap-2 sm:grid-cols-2" : "flex flex-wrap gap-2"}>
      <Button data-direct-start="invite" disabled={disabled} onClick={() => onStart("invite")}>
        {t("direct.inviteGroup")}
      </Button>
      <Button
        data-direct-start="join"
        variant="outline"
        disabled={disabled}
        onClick={() => onStart("join")}
      >
        {t("direct.useInvite")}
      </Button>
    </div>
  );
  if (!empty) return actions;
  return (
    <section
      aria-label={t("direct.startTitle")}
      className="flex flex-1 flex-col items-center justify-center gap-4 py-6 text-center"
    >
      <div
        aria-hidden="true"
        className="flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]"
      >
        <Cable className="h-6 w-6" />
      </div>
      <div className="space-y-1.5">
        <h3 className="text-base font-semibold">{t("direct.startTitle")}</h3>
        <p className="text-sm text-[var(--color-text-secondary)]">{t("direct.startHint")}</p>
      </div>
      {actions}
    </section>
  );
}

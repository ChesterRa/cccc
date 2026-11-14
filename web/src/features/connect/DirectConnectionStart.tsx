import { ArrowDownLeft, Cable, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { LcdIcon } from "../../components/Icons";

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
    <div className="grid w-full gap-3 sm:grid-cols-2">
      <Button
        data-direct-start="invite"
        aria-label={t("direct.inviteGroup")}
        disabled={disabled}
        className="h-auto min-h-20 items-start justify-start gap-3 whitespace-normal px-4 py-4 text-left"
        onClick={() => onStart("invite")}
      >
        <LcdIcon icon={Plus} glyph="plus" className="mt-0.5 h-5 w-5 shrink-0" aria-hidden="true" />
        <span className="min-w-0 space-y-1">
          <span className="block font-semibold">{t("direct.inviteGroup")}</span>
          <span className="block text-sm font-normal opacity-80">
            {t("direct.inviteActionHint")}
          </span>
        </span>
      </Button>
      <Button
        data-direct-start="join"
        variant="outline"
        aria-label={t("direct.useInvite")}
        disabled={disabled}
        className="h-auto min-h-20 items-start justify-start gap-3 whitespace-normal px-4 py-4 text-left"
        onClick={() => onStart("join")}
      >
        <LcdIcon
          icon={ArrowDownLeft}
          glyph="arrow-down"
          className="mt-0.5 h-5 w-5 shrink-0"
          aria-hidden="true"
        />
        <span className="min-w-0 space-y-1">
          <span className="block font-semibold">{t("direct.useInvite")}</span>
          <span className="block text-sm font-normal text-[var(--color-text-secondary)]">
            {t("direct.joinActionHint")}
          </span>
        </span>
      </Button>
    </div>
  );
  if (!empty) return actions;
  return (
    <section
      aria-label={t("direct.startTitle")}
      className="flex flex-col items-center justify-center gap-4 py-4 text-center"
    >
      <div
        aria-hidden="true"
        className="flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]"
      >
        <LcdIcon icon={Cable} glyph="link" className="h-6 w-6" />
      </div>
      <div className="space-y-1.5">
        <h3 className="text-base font-semibold">{t("direct.startTitle")}</h3>
        <p className="text-sm text-[var(--color-text-secondary)]">{t("direct.startHint")}</p>
      </div>
      {actions}
    </section>
  );
}

import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";

export function DirectInviteLifetime({
  value,
  onChange,
  disabled,
}: {
  value: number;
  onChange: (value: number) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation("layout");
  return (
    <div className="space-y-2">
      <p className="text-sm font-medium">{t("direct.invitationLifetime")}</p>
      <div
        role="group"
        aria-label={t("direct.invitationLifetime")}
        className="grid grid-cols-2 gap-2 sm:grid-cols-4"
      >
        {([86400, 604800, 2592000, 0] as const).map((seconds, index) => (
          <Button
            key={seconds}
            type="button"
            variant={value === seconds ? "default" : "outline"}
            aria-pressed={value === seconds}
            disabled={disabled}
            className="min-h-11 px-2"
            onClick={() => onChange(seconds)}
          >
            {t(
              [
                "direct.lifetimeDay",
                "direct.lifetimeWeek",
                "direct.lifetimeMonth",
                "direct.lifetimePermanent",
              ][index],
            )}
          </Button>
        ))}
      </div>
      <p className="text-sm text-[var(--color-text-secondary)]">{t("direct.reusableHint")}</p>
    </div>
  );
}

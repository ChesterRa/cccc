import { useTranslation } from "react-i18next";
import { useUIStore } from "../../stores";

/**
 * Points from Group connections to adding running sessions as members. The sentence ends in the
 * link, so it is shown only to admins, who are the only ones able to add those members.
 */
export function ExistingSessionsHint({ onOpen }: { onOpen: () => void }) {
  const { t } = useTranslation("layout");
  const canAdd = useUIStore((state) => state.isWebAdmin === true && !state.webReadOnly);
  if (!canAdd) return null;
  return (
    <p className="shrink-0 text-sm leading-relaxed text-[var(--color-text-secondary)]">
      {t("groupConnections.membersStaySeparate")}{" "}
      <button
        type="button"
        className="underline underline-offset-2 hover:text-[var(--color-text-primary)]"
        onClick={onOpen}
      >
        {t("groupConnections.useExistingSessions")}
      </button>
    </p>
  );
}

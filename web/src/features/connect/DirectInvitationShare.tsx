import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/textarea";
import { copyTextToClipboard } from "../../utils/copy";
import { readDirectInvitation } from "./directConnectionModel";

export function DirectInvitationShare({ text }: { text: string }) {
  const { t, i18n } = useTranslation("layout");
  const [copy, setCopy] = useState<"idle" | "copied" | "failed">("idle");
  const preview = readDirectInvitation(text);
  if (!preview) return null;
  if (Date.parse(preview.expires_at) <= Date.now()) return <p>{t("direct.expiredHint")}</p>;
  return (
    <div className="space-y-3 rounded-xl border border-[var(--glass-border-subtle)] p-3">
      <p>{t("direct.shareHint")}</p>
      <Textarea
        aria-label={t("direct.invitation")}
        rows={2}
        readOnly
        value={text}
        className="font-mono text-xs"
        onFocus={(e) => e.target.select()}
      />
      <Button
        onClick={async () => {
          const focus = document.activeElement as HTMLElement | null;
          setCopy((await copyTextToClipboard(text)) ? "copied" : "failed");
          if (focus?.isConnected) focus.focus();
        }}
      >
        {t(copy === "copied" ? "direct.copied" : "direct.copy")}
      </Button>
      {copy === "failed" && <p role="alert">{t("direct.copyFailed")}</p>}
      <p className="text-xs text-[var(--color-text-secondary)]">
        {t("direct.expiresAt", {
          time: new Date(preview.expires_at).toLocaleString(i18n.language),
        })}{" "}
        · {t("direct.keepInvitation")}
      </p>
    </div>
  );
}

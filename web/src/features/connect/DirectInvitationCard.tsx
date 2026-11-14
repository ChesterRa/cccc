import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronDown, Copy } from "lucide-react";
import { LcdIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/textarea";
import { classNames } from "../../utils/classNames";
import { copyTextToClipboard } from "../../utils/copy";
import {
  readDirectInvitation,
  directInvitationExpired,
  DIRECT_PERMANENT_EXPIRY,
} from "./directConnectionModel";
import "./directInvitationInput.css";

import { DirectInvitationSummary, type InvitationMetadata } from "./DirectInvitationSummary";

/** The bearer is read only after Copy or Show; polling supplies metadata only. */
export function DirectInvitationCard({
  invitation,
  cachedText = "",
  busy = false,
  framed = true,
  onRead,
  onDelete,
  deleteDisabled = false,
}: {
  invitation: InvitationMetadata;
  cachedText?: string;
  busy?: boolean;
  framed?: boolean;
  onRead: () => Promise<string | null>;
  onDelete?: () => void;
  deleteDisabled?: boolean;
}) {
  const { t } = useTranslation("layout");
  const [value, setValue] = useState("");
  const [revealed, setRevealed] = useState(false);
  const [pending, setPending] = useState<"copy" | "show" | null>(null);
  const [copy, setCopy] = useState<"idle" | "copied" | "failed">("idle");
  const field = useRef<HTMLTextAreaElement>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const expired =
    invitation.expired ||
    invitation.current === false ||
    directInvitationExpired(invitation.expires_at);
  const unavailable = busy || !!pending || expired;
  const read = async () => {
    const text = cachedText || (await onRead());
    if (!mounted.current) return null;
    const preview = text ? readDirectInvitation(text) : null;
    if (
      !text ||
      !preview ||
      preview.id !== invitation.id ||
      !!preview.reusable !== invitation.reusable ||
      directInvitationExpired(preview.expires_at) ||
      !!preview.permanent !== (invitation.expires_at === DIRECT_PERMANENT_EXPIRY)
    ) {
      setCopy("failed");
      return null;
    }
    setValue(text);
    return text;
  };

  return (
    <div
      className={classNames(
        "min-w-0 space-y-4",
        framed && "rounded-xl border border-[var(--glass-border-subtle)] p-4",
      )}
    >
      <DirectInvitationSummary
        invitation={invitation}
        expired={expired}
        busy={busy || !!pending}
        deleteDisabled={deleteDisabled}
        onDelete={onDelete}
      />
      <p className="text-sm text-[var(--color-text-secondary)]">
        {t(invitation.reusable ? "direct.reusableHint" : "direct.legacyHint")}
      </p>
      <div className="flex flex-wrap gap-2">
        <Button
          data-direct-copy
          disabled={unavailable}
          className="min-h-11 min-w-28 gap-2"
          onClick={async () => {
            setPending("copy");
            setCopy("idle");
            const text = await read();
            if (text && mounted.current) {
              const ok = await copyTextToClipboard(text, revealed ? field.current : undefined);
              if (mounted.current) {
                setCopy(ok ? "copied" : "failed");
                if (!ok) setRevealed(true);
              }
            }
            if (mounted.current) setPending(null);
          }}
        >
          <LcdIcon
            icon={copy === "copied" ? Check : Copy}
            glyph="copy"
            className="h-4 w-4"
            aria-hidden="true"
          />
          {t(
            pending === "copy"
              ? "direct.copying"
              : copy === "copied"
                ? "direct.copied"
                : "direct.copy",
          )}
        </Button>
        <Button
          variant="ghost"
          disabled={unavailable && !revealed}
          aria-expanded={revealed}
          className="min-h-11 gap-1.5"
          onClick={async () => {
            if (revealed) {
              setRevealed(false);
              return;
            }
            setPending("show");
            setCopy("idle");
            if (await read()) setRevealed(true);
            if (mounted.current) setPending(null);
          }}
        >
          {t(revealed ? "direct.hideInvitation" : "direct.showInvitation")}
          <LcdIcon
            icon={ChevronDown}
            glyph="chevron-down"
            aria-hidden="true"
            className={classNames(
              "h-4 w-4 transition-transform motion-reduce:transition-none",
              revealed && "rotate-180",
            )}
          />
        </Button>
      </div>
      <Textarea
        ref={field}
        hidden={!revealed}
        aria-label={t("direct.invitation")}
        rows={3}
        readOnly
        value={cachedText || value}
        className="direct-invitation-input w-full font-mono text-xs"
        onFocus={(event) => event.currentTarget.select()}
      />
      {copy === "copied" && (
        <span className="sr-only" role="status">
          {t("direct.copied")}
        </span>
      )}
      {copy === "failed" && (
        <p role="alert" className="text-sm text-[var(--color-danger)]">
          {t(value || cachedText ? "direct.copyFailed" : "direct.actionFailed")}
        </p>
      )}
    </div>
  );
}

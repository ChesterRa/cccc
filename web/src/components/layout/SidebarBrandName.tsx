import { useEffect, useRef, useState } from "react";
import type { PointerEvent } from "react";
import { useTranslation } from "react-i18next";
import { CopyIcon } from "../Icons";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { copyTextToClipboard } from "../../utils/copy";

const NAME_CLASS =
  "truncate text-[15px] font-semibold tracking-[-0.035em] text-[var(--color-text-primary)]";
// Narrow headers fold the "New" label first (at 13rem) so the brand name still fits
// at the 200 px minimum width. Only below 10.5rem (for example at 125% text size on a
// 200 px sidebar) does a plain name give way to the logo; a name that opens the
// instance card is its only entry, so it stays and is cut short instead.
const PLAIN_WRAP_CLASS = "min-w-0 flex-1 @max-[10.5rem]/brand:hidden";
const CARD_WRAP_CLASS = "min-w-0 flex-1";

/** Long enough to cross the gap from the name to the card. */
export const BRAND_CARD_CLOSE_DELAY_MS = 150;
const COPIED_FEEDBACK_MS = 1500;

/**
 * The product name in the sidebar header. When Connect knows this instance, the name
 * itself shows the instance details while a mouse is over it or the card; a tap or
 * Enter/Space toggles them where there is no hover, so no separate info key is needed.
 * A public address can be copied from the card.
 */
export function SidebarBrandName({
  productName,
  instanceLabel,
  instanceOrigin,
}: {
  productName: string;
  instanceLabel?: string;
  instanceOrigin?: string | null;
}) {
  const { t } = useTranslation("layout");
  const { t: tCommon } = useTranslation("common");
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const copiedTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const cancelClose = () => {
    if (closeTimer.current) clearTimeout(closeTimer.current);
    closeTimer.current = null;
  };
  // Hovering the name or the card keeps it open; leaving both closes it after a beat.
  const hoverOpen = (event: PointerEvent) => {
    if (event.pointerType !== "mouse") return;
    cancelClose();
    setOpen(true);
  };
  const hoverClose = (event: PointerEvent) => {
    if (event.pointerType !== "mouse") return;
    cancelClose();
    closeTimer.current = setTimeout(() => setOpen(false), BRAND_CARD_CLOSE_DELAY_MS);
  };
  useEffect(
    () => () => {
      if (closeTimer.current) clearTimeout(closeTimer.current);
      if (copiedTimer.current) clearTimeout(copiedTimer.current);
    },
    [],
  );

  if (!instanceLabel) {
    return (
      <div className={PLAIN_WRAP_CLASS}>
        <div className={NAME_CLASS} data-brand-name>
          {productName}
        </div>
      </div>
    );
  }

  const copyOrigin = async () => {
    if (!instanceOrigin || !(await copyTextToClipboard(instanceOrigin))) return;
    setCopied(true);
    if (copiedTimer.current) clearTimeout(copiedTimer.current);
    copiedTimer.current = setTimeout(() => setCopied(false), COPIED_FEEDBACK_MS);
  };
  const copyLabel = copied ? tCommon("copied") : tCommon("copy");

  return (
    <div className={CARD_WRAP_CLASS}>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            data-brand-name
            aria-label={`${productName} · ${t("sidebarInstanceInfo")}`}
            className={`${NAME_CLASS} block max-w-full cursor-default text-left focus-visible:outline-2`}
            onPointerEnter={hoverOpen}
            onPointerLeave={hoverClose}
          >
            {productName}
          </button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          data-brand-instance-info
          // Hover opens it; leave focus on the name so the pointer keeps control.
          onOpenAutoFocus={(event) => event.preventDefault()}
          onPointerEnter={hoverOpen}
          onPointerLeave={hoverClose}
          className="max-w-[min(20rem,calc(100vw-2rem))] p-3 text-xs"
        >
          <p className="break-words font-medium text-[var(--color-text-primary)]">
            {instanceLabel}
          </p>
          <div className="mt-1 flex items-start gap-2">
            <p className="min-w-0 flex-1 break-all text-[var(--color-text-secondary)]">
              {instanceOrigin || t("connect.noRemoteAccess")}
            </p>
            {instanceOrigin ? (
              <button
                type="button"
                data-brand-copy-origin
                aria-label={copyLabel}
                title={copyLabel}
                onClick={() => void copyOrigin()}
                className="flex shrink-0 items-center gap-1 rounded-md px-1 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] focus-visible:outline-2"
              >
                <CopyIcon size={13} strokeWidth={1.25} />
                {copied ? <span>{copyLabel}</span> : null}
              </button>
            ) : null}
          </div>
        </PopoverContent>
      </Popover>
    </div>
  );
}

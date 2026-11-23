import { useMemo, useRef } from "react";
import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import { LcdIcon } from "../../components/Icons";
import { Input } from "../../components/ui/input";
import type { SessionNodeDevice } from "../../services/api/sessionNodes";
import { SessionNodeDevices } from "./SessionNodeDevices";
import type { useSessionNodes } from "./useSessionNodes";

export function SessionNodeManagement({
  devices,
  actions,
  query,
  onSearch,
}: {
  devices: SessionNodeDevice[];
  actions: ReturnType<typeof useSessionNodes>;
  query: string;
  onSearch: (query: string) => void;
}) {
  const { t, i18n } = useTranslation("layout");
  const scroller = useRef<HTMLDivElement>(null);
  const names = useMemo(() => new Intl.Collator(i18n.language, { numeric: true }), [i18n.language]);
  const term = query.trim().normalize("NFKC").toLocaleLowerCase(i18n.language);
  const matches = devices
    .filter((device) =>
      device.name.normalize("NFKC").toLocaleLowerCase(i18n.language).includes(term),
    )
    .sort((a, b) => names.compare(a.name, b.name) || a.node_id.localeCompare(b.node_id));
  if (!devices.length)
    return (
      <p role="status" className="py-2 text-sm text-[var(--color-text-secondary)]">
        {t("sessionNodes.noPairedDevices")}
      </p>
    );
  return (
    <div className="min-w-0 space-y-2">
      <label className="relative block">
        <LcdIcon
          icon={Search}
          glyph="search"
          size={16}
          aria-hidden="true"
          className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-[var(--color-text-tertiary)]"
        />
        <Input
          type="search"
          value={query}
          onChange={(e) => {
            onSearch(e.target.value);
            actions.setRevokeId("");
            if (scroller.current) scroller.current.scrollTop = 0;
          }}
          aria-label={t("sessionNodes.searchDevices")}
          placeholder={t("sessionNodes.searchDevices")}
          className="min-h-10 pl-9 max-sm:min-h-11"
        />
      </label>
      <p role="status" className="text-xs text-[var(--color-text-secondary)]">
        {t("sessionNodes.deviceCount", { shown: matches.length, total: devices.length })}
      </p>
      {matches.length ? (
        <div
          ref={scroller}
          tabIndex={0}
          role="region"
          aria-label={t("sessionNodes.devices")}
          className="max-h-[min(22rem,42dvh)] overflow-y-auto overscroll-contain border-y border-[var(--glass-border-subtle)] px-1 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
        >
          <SessionNodeDevices
            devices={matches}
            busy={actions.revoking}
            revokeId={actions.revokeId}
            setRevokeId={actions.setRevokeId}
            revoke={actions.revoke}
          />
        </div>
      ) : (
        <p role="status" className="py-2 text-sm text-[var(--color-text-secondary)]">
          {t("sessionNodes.noMatchingDevices")}
        </p>
      )}
      {actions.revokeError && (
        <p role="alert" className="text-sm text-[var(--color-accent-danger)]">
          {t("sessionNodes.revokeFailed")}
        </p>
      )}
    </div>
  );
}

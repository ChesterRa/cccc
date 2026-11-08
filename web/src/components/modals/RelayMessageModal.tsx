import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Actor, ChatMessageData, GroupMeta, LedgerEvent } from "../../types";
import { classNames } from "../../utils/classNames";
import { MOBILE_VIEWPORT_MEDIA_QUERY } from "../../utils/responsiveLayout";
import * as api from "../../services/api";
import { useModalA11y } from "../../hooks/useModalA11y";
import { useChatObscuringViewOnPhone } from "../../hooks/useChatObscuringView";
import { Button } from "../ui/button";
import { Surface } from "../ui/surface";
import { Textarea } from "../ui/textarea";
import { GroupCombobox } from "../GroupCombobox";

const GROUP_RECIPIENTS = ["@all", "@foreman", "@peers"];

export interface RelayMessageModalProps {
  isOpen: boolean;
  busy: boolean;
  srcGroupId: string;
  srcEvent: LedgerEvent | null;
  groups: GroupMeta[];
  onCancel: () => void;
  onSubmit: (dstGroupId: string, to: string[], note: string) => void;
}

export function RelayMessageModal({
  isOpen,
  busy,
  srcGroupId,
  srcEvent,
  groups,
  onCancel,
  onSubmit,
}: RelayMessageModalProps) {
  const { t } = useTranslation("modals");
  useChatObscuringViewOnPhone(isOpen);
  const dstGroups = useMemo(() => {
    return (groups || [])
      .map((g) => {
        const groupId = String(g.group_id || "").trim();
        const title = String(g.title || "").trim();
        const topic = String(g.topic || "").trim();
        const label = title || topic || groupId;
        return {
          value: groupId,
          label,
          description: label !== groupId ? groupId : undefined,
          keywords: [groupId, title, topic].filter(Boolean),
        };
      })
      .filter((g) => g.value && g.value !== srcGroupId);
  }, [groups, srcGroupId]);

  const defaultDstGroupId = useMemo(() => {
    return dstGroups[0]?.value || "";
  }, [dstGroups]);

  const [dstGroupId, setDstGroupId] = useState(() => defaultDstGroupId);
  const [dstActors, setDstActors] = useState<Actor[]>([]);
  const [dstActorsLoadingFor, setDstActorsLoadingFor] = useState(() => defaultDstGroupId);
  const [note, setNote] = useState("");
  const [toTokens, setToTokens] = useState<string[]>(["@all"]);
  const [recipientsNeedReview, setRecipientsNeedReview] = useState(false);
  const actorsRequestEpochRef = useRef(0);

  const srcEventId = srcEvent?.id ? String(srcEvent.id) : "";
  const srcBy = srcEvent?.by ? String(srcEvent.by) : "";
  const srcText = useMemo(() => {
    const d = srcEvent?.data as ChatMessageData | undefined;
    const t = d?.text;
    return typeof t === "string" ? t : "";
  }, [srcEvent]);
  const srcQuoteText = useMemo(() => {
    const d = srcEvent?.data as ChatMessageData | undefined;
    const t = d?.quote_text;
    return typeof t === "string" ? t : "";
  }, [srcEvent]);

  useEffect(() => {
    if (!isOpen) return;
    const gid = String(dstGroupId || "").trim();
    if (!gid) return;
    const epoch = actorsRequestEpochRef.current + 1;
    actorsRequestEpochRef.current = epoch;
    let cancelled = false;
    void api
      .fetchActors(gid, false)
      .then((resp) => {
        if (cancelled || actorsRequestEpochRef.current !== epoch) return;
        if (!resp.ok) {
          setDstActors([]);
          return;
        }
        setDstActors(resp.result.actors || []);
      })
      .finally(() => {
        if (cancelled || actorsRequestEpochRef.current !== epoch) return;
        setDstActorsLoadingFor("");
      });
    return () => {
      cancelled = true;
    };
  }, [dstGroupId, isOpen]);

  const dstActorsBusy = !!dstGroupId && dstActorsLoadingFor === dstGroupId;

  const availableTokens = useMemo(() => {
    const actorIds = (dstActors || []).map((a) => String(a.id || "")).filter((id) => id);
    actorIds.sort();
    return [...GROUP_RECIPIENTS, ...actorIds];
  }, [dstActors]);

  const toggleToken = (token: string) => {
    const t = token.trim();
    if (!t) return;
    setRecipientsNeedReview(false);
    setToTokens((prev) => {
      const exists = prev.includes(t);
      if (exists) return prev.filter((x) => x !== t);
      return prev.concat([t]);
    });
  };

  const { modalRef } = useModalA11y(isOpen, onCancel);

  useEffect(() => {
    if (!isOpen || !window.matchMedia(MOBILE_VIEWPORT_MEDIA_QUERY).matches) return;
    // The modal's initial focus can leave the selected audience below the fixed footer. Reveal
    // it only when the destination and selection can both stay in view; very short screens
    // keep the focused destination visible and let the user scroll the form normally.
    const frame = requestAnimationFrame(() => {
      const modal = modalRef.current;
      const scroll = modal?.querySelector<HTMLElement>("[data-relay-scroll]");
      const group = modal?.querySelector<HTMLButtonElement>('button[role="combobox"]');
      const selected = modal?.querySelector<HTMLButtonElement>(
        '[data-relay-recipient-row] button[aria-pressed="true"]',
      );
      if (!scroll || !group || !selected || document.activeElement !== group) return;
      const viewport = scroll.getBoundingClientRect();
      // The opening animation scales visual rectangles, while scrollTop stays in CSS pixels.
      const scale = viewport.height / scroll.clientHeight;
      if (!Number.isFinite(scale) || scale <= 0) return;
      const missing = (selected.getBoundingClientRect().bottom - viewport.bottom) / scale;
      const room = (group.getBoundingClientRect().top - viewport.top) / scale;
      const distance = Math.ceil(missing) + 1;
      if (missing > 0.5 && distance <= room) scroll.scrollTop += distance;
    });
    return () => cancelAnimationFrame(frame);
  }, [isOpen, modalRef]);

  if (!isOpen) return null;

  const canSubmit =
    !!dstGroupId && !!srcEventId && !!(srcText || note).trim() && !busy && !recipientsNeedReview;

  return (
    <div
      className="fixed inset-0 backdrop-blur-sm flex items-stretch sm:items-start justify-center p-0 sm:p-6 z-50 animate-fade-in glass-overlay"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="relay-modal-title"
    >
      <div
        ref={modalRef}
        className="w-full h-full min-h-0 sm:h-auto sm:max-h-[calc(100dvh-7rem)] sm:max-w-2xl sm:mt-14 shadow-2xl animate-scale-in overflow-hidden flex flex-col rounded-none sm:rounded-2xl glass-modal"
      >
        <div className="px-6 py-4 border-b safe-area-inset-top border-[var(--glass-border-subtle)]">
          <div
            id="relay-modal-title"
            className="text-lg font-semibold text-[var(--color-text-primary)]"
          >
            {t("relay.title")}
          </div>
          <div className="text-xs mt-1 text-[var(--color-text-muted)]">{t("relay.subtitle")}</div>
        </div>

        <div className="p-6 flex flex-1 min-h-0 flex-col gap-5">
          <div data-relay-scroll className="min-h-0 flex-1 overflow-y-auto">
            <div className="space-y-5">
              {/* Keep the source preview and form reachable when the viewport is short. */}
              <Surface className="overflow-hidden" padding="md" radius="md">
                <div className="text-xs font-semibold text-[var(--color-text-primary)]">
                  {t("relay.source")}
                </div>
                <div className="text-[11px] mt-1 break-words text-[var(--color-text-muted)]">
                  {srcGroupId} · {srcEventId ? srcEventId : "—"} · {srcBy || "—"}
                </div>
                <div className="mt-2 max-h-[min(40vh,20rem)] overflow-x-hidden overflow-y-auto whitespace-pre-wrap break-words pr-1 text-sm text-[var(--color-text-primary)]">
                  {srcQuoteText ? (
                    <div className="mb-3 border-l-2 border-[var(--glass-border-subtle)] pl-3 text-[var(--color-text-secondary)]">
                      "{srcQuoteText}"
                    </div>
                  ) : null}
                  {srcText || t("relay.emptyMessage")}
                </div>
              </Surface>

              <div className="space-y-5 pr-1">
                {/* Destination group */}
                <div>
                  <label className="block text-xs font-medium mb-2 text-[var(--color-text-secondary)]">
                    {t("relay.destinationGroup")}
                  </label>
                  <GroupCombobox
                    items={dstGroups}
                    value={dstGroupId}
                    onChange={(gid) => {
                      if (gid === dstGroupId) return;
                      if (toTokens.some((token) => !GROUP_RECIPIENTS.includes(token))) {
                        setToTokens([]);
                        setRecipientsNeedReview(true);
                      }
                      setDstGroupId(gid);
                      setDstActors([]);
                      setDstActorsLoadingFor(gid);
                    }}
                    placeholder={t("relay.destinationGroup")}
                    searchPlaceholder={t("relay.searchDestinationGroup")}
                    emptyText={t("relay.noMatchingGroups")}
                    ariaLabel={t("relay.destinationGroup")}
                    triggerClassName="glass-input min-h-[44px] w-full px-3 py-2.5 text-sm"
                    contentClassName="w-[var(--radix-popover-trigger-width)]"
                    disabled={busy || dstGroups.length === 0}
                  />
                  {dstGroups.length === 0 ? (
                    <div className="mt-2 text-xs text-[var(--color-text-muted)]">
                      {t("relay.noOtherGroups")}
                    </div>
                  ) : null}
                  {recipientsNeedReview ? (
                    <div
                      className="mt-2 whitespace-pre-line break-words text-xs text-amber-700 dark:text-amber-300"
                      data-status-tone="warning"
                      role="status"
                    >
                      {t("relay.reselectRecipients")}
                    </div>
                  ) : null}
                </div>

                {/* Recipients */}
                <div>
                  <div className="flex items-center justify-between gap-3">
                    <label className="block text-xs font-medium text-[var(--color-text-secondary)]">
                      {t("relay.recipients")}
                    </label>
                    <Button
                      type="button"
                      className="h-auto min-h-0 px-0 py-0 text-xs underline text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]"
                      variant="ghost"
                      size="sm"
                      data-lcd-key
                      onClick={() => {
                        setToTokens([]);
                        setRecipientsNeedReview(false);
                      }}
                      disabled={busy}
                    >
                      {t("common:reset")}
                    </Button>
                  </div>

                  <div
                    data-relay-recipient-row
                    className={classNames(
                      "mt-2 flex flex-wrap gap-2",
                      dstActorsBusy ? "opacity-60" : "",
                    )}
                  >
                    {availableTokens.map((tok) => {
                      const active = toTokens.includes(tok);
                      return (
                        <button
                          key={tok}
                          type="button"
                          aria-pressed={active}
                          data-lcd-key
                          className={classNames(
                            "min-w-0 max-w-full break-all text-xs px-2.5 py-1.5 rounded-full border transition-colors",
                            active
                              ? "bg-[var(--glass-accent-bg)] border-[var(--glass-accent-border)] text-[var(--color-accent-primary)]"
                              : "bg-[var(--glass-tab-bg)] border-[var(--glass-border-subtle)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
                          )}
                          onClick={() => toggleToken(tok)}
                          disabled={busy}
                          title={tok}
                        >
                          {tok}
                        </button>
                      );
                    })}
                  </div>
                  {!recipientsNeedReview ? (
                    <div className="mt-2 break-words text-xs text-[var(--color-text-muted)]">
                      {toTokens.length
                        ? t("relay.selectedTokens", { tokens: toTokens.join(", ") })
                        : t("relay.selectedDefault")}
                    </div>
                  ) : null}
                </div>

                {/* Optional note */}
                <div>
                  <label className="block text-xs font-medium mb-2 text-[var(--color-text-secondary)]">
                    {t("relay.noteLabel")}
                  </label>
                  <Textarea
                    value={note}
                    onChange={(e) => setNote(e.target.value)}
                    rows={3}
                    className="px-3 py-2.5"
                    placeholder={t("relay.notePlaceholder")}
                    disabled={busy}
                  />
                </div>
              </div>
            </div>
          </div>

          <div className="shrink-0 flex gap-3 pt-1 flex-wrap justify-end">
            <Button variant="secondary" onClick={onCancel} disabled={busy}>
              {t("common:cancel")}
            </Button>
            <Button onClick={() => onSubmit(dstGroupId, toTokens, note)} disabled={!canSubmit}>
              {busy ? t("relay.sending") : t("relay.relay")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

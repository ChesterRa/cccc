import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { MessageSquareTextIcon } from "../../components/Icons";
import { effectiveReplyTemplates, templatePreview } from "../../features/quickReplies/effective";
import {
  Popover,
  PopoverAnchor,
  PopoverContent,
  PopoverTrigger,
} from "../../components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "../../components/ui/command";
import {
  loadQuickReplyTemplates,
  useQuickReplyTemplateStore,
} from "../../features/quickReplies/templateStore";
import { fetchWebAccessSession } from "../../services/api/webAccess";
import { useModalStore } from "../../stores/useModalStore";
import { classNames } from "../../utils/classNames";

type Props = {
  disabled: boolean;
  className?: string;
  /** Where the list opens: above the input, near the insertion point (see the mention menu). */
  getAnchorRect: () => DOMRect | null;
  /** A phone-width list spans the anchor instead of keeping the desktop width. */
  wide: boolean;
  onRememberSelection: () => void;
  onInsert: (text: string) => void;
};

export function ComposerTemplatePicker({
  disabled,
  className,
  getAnchorRect,
  wide,
  onRememberSelection,
  onInsert,
}: Props) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const [canManage, setCanManage] = useState(false);
  const replies = useQuickReplyTemplateStore((state) => state.replies);
  const loaded = useQuickReplyTemplateStore((state) => state.loaded);
  const loadError = useQuickReplyTemplateStore((state) => state.loadError);
  const epoch = useQuickReplyTemplateStore((state) => state.epoch);
  const openSettingsTarget = useModalStore((state) => state.openSettingsTarget);
  const inserted = useRef(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  // The list opens near where the text goes, not at the button; the button stays
  // the visible entry. Without a measurable input it falls back to the button.
  const virtualRef = useMemo(
    () => ({
      current: {
        getBoundingClientRect: () =>
          getAnchorRect() ?? triggerRef.current?.getBoundingClientRect() ?? new DOMRect(),
        get contextElement() {
          return triggerRef.current ?? undefined;
        },
      },
    }),
    [getAnchorRect],
  );

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void loadQuickReplyTemplates();
    // Match the personal settings tab's access rule. A failed probe leaves
    // insertion available, without offering a settings destination we cannot open.
    setCanManage(false);
    void fetchWebAccessSession()
      .then((response) => {
        if (cancelled) return;
        const session = response.ok ? response.result.web_access_session : null;
        setCanManage(
          Boolean(
            session &&
            ((session.can_access_global_settings ?? !session.login_active) ||
              session.current_browser_signed_in),
          ),
        );
      })
      .catch(() => {
        if (!cancelled) setCanManage(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, epoch]);

  const query = filter.toLocaleLowerCase();
  const shown = (loaded ? effectiveReplyTemplates(t, replies) : []).filter((template) =>
    template.name.toLocaleLowerCase().includes(query),
  );
  const choose = (text: string) => {
    inserted.current = true;
    setOpen(false);
    onInsert(text);
  };

  return (
    <Popover
      open={open && !disabled}
      onOpenChange={(next) => {
        if (next) {
          onRememberSelection();
          inserted.current = false;
          setFilter("");
        }
        setOpen(next);
      }}
    >
      <PopoverTrigger asChild>
        <button
          ref={triggerRef}
          type="button"
          data-composer-templates
          className={className}
          aria-label={t("replyTemplates.button")}
          title={t("replyTemplates.button")}
          disabled={disabled}
          onPointerDown={onRememberSelection}
        >
          <MessageSquareTextIcon size={18} />
        </button>
      </PopoverTrigger>
      <PopoverAnchor virtualRef={virtualRef} />
      <PopoverContent
        side="top"
        align="start"
        sideOffset={12}
        // Never flip below the anchor, which would cover the draft; a short viewport
        // makes the list scroll instead, clear of the window edge.
        avoidCollisions={false}
        collisionPadding={8}
        data-lcd-menu
        className={classNames(
          "flex flex-col p-0",
          wide ? "max-w-none" : "w-80 max-w-[calc(100vw-1rem)]",
        )}
        style={wide ? { width: "var(--radix-popover-trigger-width)" } : undefined}
        onCloseAutoFocus={(event) => {
          if (inserted.current) event.preventDefault();
        }}
      >
        <Command shouldFilter={false} className="flex min-h-0 flex-1 flex-col">
          <CommandInput
            value={filter}
            onValueChange={setFilter}
            placeholder={t("replyTemplates.filter")}
            aria-label={t("replyTemplates.filter")}
          />
          <CommandList
            aria-label={t("replyTemplates.button")}
            // Two-line rows: room for the four defaults and a few of your own at a glance.
            className="min-h-0 max-h-[min(30rem,calc(100dvh-12rem))] flex-1"
          >
            {loaded ? (
              <CommandEmpty>{t("replyTemplates.none")}</CommandEmpty>
            ) : (
              <div className="px-3 py-3 text-sm text-[var(--color-text-muted)]">
                {loadError ? t("replyTemplates.unavailable") : t("common:loading")}
              </div>
            )}
            {shown.length > 0 && (
              // One list in the person's saved order; the default replies are ordinary entries.
              <CommandGroup>
                {shown.map((template) => (
                  <CommandItem
                    key={template.source}
                    value={template.source}
                    onSelect={() => choose(template.text)}
                    className="flex-col items-stretch gap-0.5"
                  >
                    <TemplateRow name={template.name} text={template.text} />
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
        {canManage && (
          <div className="border-t border-[var(--glass-border-subtle)] p-1.5">
            <button
              type="button"
              className="w-full rounded-xl px-3 py-2 text-left text-sm text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]"
              onClick={() => {
                inserted.current = true;
                setOpen(false);
                openSettingsTarget({ scope: "global", tab: "quickReplies" });
              }}
            >
              {t("replyTemplates.manage")}
            </button>
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}

function TemplateRow({ name, text }: { name: string; text: string }) {
  const preview = templatePreview(text);
  return (
    <>
      <span className="truncate leading-5" data-template-name>
        {name}
      </span>
      {preview ? (
        <span
          className="truncate text-xs leading-4 text-[var(--color-text-muted)]"
          data-lcd-hint
          data-template-preview
        >
          {preview}
        </span>
      ) : null}
    </>
  );
}

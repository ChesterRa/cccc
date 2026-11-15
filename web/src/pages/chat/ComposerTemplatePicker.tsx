import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { MessageSquareTextIcon } from "../../components/Icons";
import { QUICK_REPLY_KINDS } from "../../components/messageBubble/quickReplies";
import { Popover, PopoverContent, PopoverTrigger } from "../../components/ui/popover";
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

type Props = {
  disabled: boolean;
  className?: string;
  onRememberSelection: () => void;
  onInsert: (text: string) => void;
};

export function ComposerTemplatePicker({
  disabled,
  className,
  onRememberSelection,
  onInsert,
}: Props) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const [canManage, setCanManage] = useState(false);
  const templates = useQuickReplyTemplateStore((state) => state.templates);
  const epoch = useQuickReplyTemplateStore((state) => state.epoch);
  const openSettingsTarget = useModalStore((state) => state.openSettingsTarget);
  const inserted = useRef(false);

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
  const matches = (name: string) => name.toLocaleLowerCase().includes(query);
  const builtIn = QUICK_REPLY_KINDS.map((kind) => ({
    id: kind,
    name: t(`quickReply.${kind}`),
    text: t(`quickReply.${kind}Text`),
  })).filter((template) => matches(template.name));
  const mine = templates.filter((template) => matches(template.name));
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
      <PopoverContent
        side="top"
        align="start"
        className="w-72 max-w-[calc(100vw-1rem)] p-0"
        onCloseAutoFocus={(event) => {
          if (inserted.current) event.preventDefault();
        }}
      >
        <Command shouldFilter={false}>
          <CommandInput
            value={filter}
            onValueChange={setFilter}
            placeholder={t("replyTemplates.filter")}
            aria-label={t("replyTemplates.filter")}
          />
          <CommandList aria-label={t("replyTemplates.button")}>
            <CommandEmpty>{t("replyTemplates.none")}</CommandEmpty>
            {builtIn.length > 0 && (
              <CommandGroup heading={t("replyTemplates.builtIn")}>
                {builtIn.map((template) => (
                  <CommandItem
                    key={template.id}
                    value={`builtin:${template.id}`}
                    onSelect={() => choose(template.text)}
                  >
                    <span className="truncate">{template.name}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            )}
            {mine.length > 0 && (
              <CommandGroup heading={t("replyTemplates.mine")}>
                {mine.map((template) => (
                  <CommandItem
                    key={template.id}
                    value={`custom:${template.id}`}
                    onSelect={() => choose(template.text)}
                  >
                    <span className="truncate">{template.name}</span>
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

import type { ReactNode } from "react";
import { classNames } from "../../utils/classNames";
import { ActorAvatar } from "../ActorAvatar";

export function MessageMetadataHeader({
  mobile,
  isUserMessage,
  isDark,
  senderAccentTextClass,
  senderDisplayName,
  messageTimestamp,
  fullMessageTimestamp,
  senderAvatarUrl,
  senderRuntime,
  avatarRingClassName,
  remoteBadgeLabel,
  children,
}: {
  mobile?: boolean;
  isUserMessage: boolean;
  isDark: boolean;
  senderAccentTextClass?: string | null;
  senderDisplayName: string;
  messageTimestamp: string;
  fullMessageTimestamp: string;
  senderAvatarUrl?: string;
  senderRuntime?: string;
  avatarRingClassName?: string;
  remoteBadgeLabel?: string;
  children?: ReactNode;
}) {
  const senderTextClass = isUserMessage
    ? isDark
      ? "text-slate-300"
      : "text-gray-700"
    : senderAccentTextClass
      ? senderAccentTextClass
      : isDark
        ? "text-slate-300"
        : "text-gray-700";

  if (mobile) {
    return (
      <div
        className={classNames(
          "mb-1 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs sm:hidden",
          isUserMessage ? "justify-end" : "justify-start",
        )}
      >
        <ActorAvatar
          avatarUrl={senderAvatarUrl}
          runtime={senderRuntime}
          title={senderDisplayName}
          isUser={isUserMessage}
          isDark={isDark}
          accentRingClassName={avatarRingClassName}
          sizeClassName="h-6 w-6"
          textClassName="text-xs"
        />
        <span
          className={classNames(
            "min-w-0 truncate text-xs font-medium",
            senderTextClass,
            isUserMessage && !remoteBadgeLabel && "sr-only",
          )}
          title={senderDisplayName}
        >
          {senderDisplayName}
        </span>
        {remoteBadgeLabel ? (
          <span
            className="shrink-0 rounded-full border border-emerald-300/70 bg-emerald-50/85 px-1.5 py-0.5 text-xs font-semibold leading-none text-emerald-800 dark:border-emerald-300/30 dark:bg-emerald-950/35 dark:text-emerald-100"
            title={remoteBadgeLabel}
          >
            {remoteBadgeLabel}
          </span>
        ) : null}
        {children}
        <span className="shrink-0 text-xs text-[var(--color-text-tertiary)]">
          <span title={fullMessageTimestamp}>{messageTimestamp}</span>
        </span>
      </div>
    );
  }

  return (
    <div className="mb-1 hidden min-w-0 flex-wrap items-center gap-x-2 gap-y-1 px-1 text-xs sm:flex">
      <span
        className={classNames(
          "min-w-0 truncate text-xs font-semibold tracking-[0.01em]",
          isUserMessage && !remoteBadgeLabel && "sr-only",
          isUserMessage
            ? isDark
              ? "text-[var(--color-text-secondary)]"
              : "text-gray-500"
            : senderAccentTextClass
              ? senderAccentTextClass
              : isDark
                ? "text-[var(--color-text-secondary)]"
                : "text-gray-500",
        )}
      >
        {senderDisplayName}
      </span>
      {remoteBadgeLabel ? (
        <span
          className="shrink-0 rounded-full border border-emerald-300/70 bg-emerald-50/80 px-1.5 py-0.5 text-xs font-semibold leading-none text-emerald-800 dark:border-emerald-300/30 dark:bg-emerald-950/35 dark:text-emerald-100"
          title={remoteBadgeLabel}
        >
          {remoteBadgeLabel}
        </span>
      ) : null}
      {children}
      <span className="shrink-0 text-xs text-[var(--color-text-tertiary)]">
        <span title={fullMessageTimestamp}>{messageTimestamp}</span>
      </span>
    </div>
  );
}

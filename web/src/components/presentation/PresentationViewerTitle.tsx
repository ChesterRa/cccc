/** Modal title; phones also show the card type and time here instead of a separate meta row. */
export function PresentationViewerTitle({
  title,
  typeLabel,
  publishedAt,
}: {
  title: string;
  typeLabel: string;
  publishedAt: string;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <span className="truncate max-sm:text-[15px] max-sm:font-semibold">{title}</span>
      {typeLabel ? (
        <span
          className="flex items-center gap-1.5 text-xs text-[var(--color-text-muted)] sm:hidden"
          data-presentation-title-meta
        >
          <span className="rounded-md bg-[var(--glass-tab-bg)] px-1.5 text-[11px] font-semibold text-[var(--color-text-secondary)]">
            {typeLabel}
          </span>
          {publishedAt ? <span className="truncate">{publishedAt}</span> : null}
        </span>
      ) : null}
    </div>
  );
}

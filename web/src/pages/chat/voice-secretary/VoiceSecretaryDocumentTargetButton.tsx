import { FileCheckIcon, FileTextIcon } from "../../../components/Icons";
import { classNames } from "../../../utils/classNames";

type VoiceSecretaryDocumentTargetButtonProps = {
  disabled: boolean;
  isDark: boolean;
  label: string;
  selected: boolean;
  onActivate: () => void;
};

export function VoiceSecretaryDocumentTargetButton({
  disabled,
  isDark,
  label,
  selected,
  onActivate,
}: VoiceSecretaryDocumentTargetButtonProps) {
  return (
    <button
      type="button"
      data-voice-document-target
      data-state={selected ? "default" : "available"}
      aria-pressed={selected}
      aria-label={label}
      disabled={disabled}
      onClick={(event) => {
        event.stopPropagation();
        if (!selected) onActivate();
      }}
      onKeyDown={(event) => event.stopPropagation()}
      className={classNames(
        "flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border transition-colors duration-200 focus-visible:outline-none focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-40",
        selected
          ? isDark
            ? "border-white/20 bg-white/[0.13] text-white shadow-[0_8px_24px_-16px_rgba(0,0,0,0.8)] hover:bg-white/[0.16] focus-visible:ring-white/40"
            : "border-black/20 bg-[rgb(35,36,37)] text-white shadow-[0_8px_22px_-15px_rgba(15,23,42,0.72)] hover:bg-[rgb(28,29,30)] focus-visible:ring-black/30"
          : isDark
            ? "border-white/10 bg-white/[0.045] text-slate-400 shadow-[0_4px_18px_-14px_rgba(0,0,0,0.85)] hover:border-white/20 hover:bg-white/[0.08] hover:text-slate-100 focus-visible:ring-white/35"
            : "border-black/10 bg-white text-gray-500 shadow-[0_4px_16px_-14px_rgba(15,23,42,0.45)] hover:border-black/20 hover:bg-[rgb(248,248,248)] hover:text-gray-800 focus-visible:ring-black/25",
      )}
    >
      {selected ? (
        <FileCheckIcon size={18} aria-hidden="true" />
      ) : (
        <FileTextIcon size={18} aria-hidden="true" />
      )}
    </button>
  );
}

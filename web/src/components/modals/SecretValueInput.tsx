import { Eye, EyeOff } from "lucide-react";

import { Input } from "../ui/input";

interface SecretValueInputProps {
  id: string;
  ariaLabel: string;
  value: string;
  visible: boolean;
  disabled: boolean;
  placeholder: string;
  showLabel: string;
  hideLabel: string;
  onChange: (value: string) => void;
  onToggleVisibility: () => void;
}

export function SecretValueInput({
  id,
  ariaLabel,
  value,
  visible,
  disabled,
  placeholder,
  showLabel,
  hideLabel,
  onChange,
  onToggleVisibility,
}: SecretValueInputProps) {
  const visibilityLabel = visible ? hideLabel : showLabel;
  const VisibilityIcon = visible ? EyeOff : Eye;
  return (
    <div className="relative min-w-0 flex-1">
      <Input
        id={id}
        aria-label={ariaLabel}
        type={visible ? "text" : "password"}
        value={value}
        disabled={disabled}
        placeholder={placeholder}
        autoComplete="new-password"
        className="pr-11 font-mono"
        onChange={(event) => onChange(event.target.value)}
      />
      <button
        type="button"
        aria-label={visibilityLabel}
        title={visibilityLabel}
        disabled={disabled}
        onClick={onToggleVisibility}
        className="absolute inset-y-0 right-1 my-auto grid h-9 w-9 place-items-center rounded-lg text-[var(--color-text-muted)] transition-colors hover:bg-[var(--glass-tab-bg)] hover:text-[var(--color-text-primary)] disabled:opacity-50"
      >
        <VisibilityIcon size={16} aria-hidden="true" />
      </button>
    </div>
  );
}

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Ellipsis, Maximize, Sparkles } from "lucide-react";
import { Button } from "../../../components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../../../components/ui/popover";
import type { VoiceSecretaryCaptureMode } from "../VoiceSecretaryComposerControl";
import "./voiceMobileControls.css";

type Choice = { value: string; label: string };

type Props = {
  disabled: boolean;
  settingsLocked: boolean;
  assistantEnabled: boolean;
  mode: VoiceSecretaryCaptureMode;
  modes: Array<{ key: VoiceSecretaryCaptureMode; label: string }>;
  onModeChange?: (mode: VoiceSecretaryCaptureMode) => void;
  language: string;
  languageDisabled: boolean;
  languages: Choice[];
  onLanguageChange: (language: string) => void;
  optimizeLabel: string;
  optimizeDisabled: boolean;
  onOptimize: () => void;
  workspaceLabel: string;
  onWorkspace: () => void;
};

export function VoiceMobileMenu(props: Props) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const choosingAction = useRef(false);
  useEffect(() => {
    if (props.disabled) setOpen(false);
  }, [props.disabled]);
  useEffect(() => {
    const trigger = triggerRef.current;
    if (!open || !trigger) return;
    const observer = new ResizeObserver(() => {
      if (!trigger.getClientRects().length) setOpen(false);
    });
    observer.observe(trigger);
    return () => observer.disconnect();
  }, [open]);
  const label = t("voiceSecretaryMobileOptions", { defaultValue: "Voice options" });
  const choose = (action: () => void) => {
    if (props.disabled) return;
    // Workspace and Document can open a modal. Give it a stable return target
    // and let it own focus after this popover closes.
    choosingAction.current = true;
    triggerRef.current?.focus();
    setOpen(false);
    action();
  };
  return (
    <div className="voice-mobile-only">
      <Popover
        open={open && !props.disabled}
        onOpenChange={(value) => {
          choosingAction.current = false;
          setOpen(value);
        }}
      >
        <PopoverTrigger asChild>
          <Button
            ref={triggerRef}
            variant="ghost"
            size="iconTouch"
            disabled={props.disabled}
            aria-label={label}
          >
            <Ellipsis size={18} aria-hidden="true" />
          </Button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          side="top"
          collisionPadding={12}
          className="voice-mobile-menu"
          aria-label={label}
          onCloseAutoFocus={(event) => {
            if (choosingAction.current) event.preventDefault();
          }}
        >
          {props.assistantEnabled && props.onModeChange ? (
            <fieldset disabled={props.disabled || props.settingsLocked} className="min-w-0">
              <legend className="px-2 text-xs text-[var(--color-text-muted)]">
                {t("voiceSecretaryModeSelector")}
              </legend>
              {props.modes.map((mode) => (
                <Button
                  key={mode.key}
                  variant="ghost"
                  className="voice-mobile-choice"
                  aria-pressed={props.mode === mode.key}
                  onClick={() => choose(() => props.onModeChange?.(mode.key))}
                >
                  {mode.label}
                  {props.mode === mode.key ? " ✓" : ""}
                </Button>
              ))}
            </fieldset>
          ) : null}
          {props.assistantEnabled ? (
            <fieldset disabled={props.disabled || props.languageDisabled} className="min-w-0">
              <legend className="px-2 text-xs text-[var(--color-text-muted)]">
                {t("voiceSecretaryLanguage")}
              </legend>
              {props.languages.map((language) => (
                <Button
                  key={language.value}
                  variant="ghost"
                  className="voice-mobile-choice"
                  aria-pressed={props.language === language.value}
                  onClick={() => choose(() => props.onLanguageChange(language.value))}
                >
                  {language.label}
                  {props.language === language.value ? " ✓" : ""}
                </Button>
              ))}
            </fieldset>
          ) : null}
          {props.assistantEnabled && props.mode === "prompt" ? (
            <Button
              variant="ghost"
              className="voice-mobile-choice"
              disabled={props.optimizeDisabled}
              onClick={() => choose(props.onOptimize)}
            >
              <Sparkles size={16} className="shrink-0" />
              {props.optimizeLabel}
            </Button>
          ) : null}
          <Button
            variant="ghost"
            className="voice-mobile-choice"
            onClick={() => choose(props.onWorkspace)}
          >
            <Maximize size={16} className="shrink-0" />
            {props.workspaceLabel}
          </Button>
        </PopoverContent>
      </Popover>
    </div>
  );
}

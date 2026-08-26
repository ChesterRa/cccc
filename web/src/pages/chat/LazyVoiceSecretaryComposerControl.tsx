import { lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";
import { MicrophoneIcon } from "../../components/Icons";
import { classNames } from "../../utils/classNames";
import { recoverDynamicImportError } from "../../utils/vitePreloadRecovery";
import type {
  VoiceSecretaryCaptureMode,
  VoiceSecretaryComposerControlProps,
} from "./VoiceSecretaryComposerControl";

function VoiceSecretaryLoadFailure(props: VoiceSecretaryComposerControlProps) {
  const { t } = useTranslation("chat");
  const assistantRow = props.variant === "assistantRow";
  const label = t("voiceSecretaryLoadFailed", {
    defaultValue: "Voice controls failed to load. Reload the page to retry.",
  });
  return (
    <button
      type="button"
      className={classNames(
        "glass-btn inline-flex items-center justify-center rounded-lg text-[var(--color-danger)] transition-colors",
        assistantRow ? "h-11 w-24 gap-1.5 text-[11px] font-semibold sm:h-9" : "h-11 w-11",
      )}
      aria-label={label}
      title={label}
      onClick={() => window.location.reload()}
    >
      <MicrophoneIcon size={15} aria-hidden="true" />
      {assistantRow ? t("voiceSecretaryReloadShort", { defaultValue: "Reload" }) : null}
    </button>
  );
}

async function loadVoiceSecretaryComposerControl() {
  try {
    const module = await import("./VoiceSecretaryComposerControl");
    return { default: module.VoiceSecretaryComposerControl };
  } catch (error) {
    const recovered = recoverDynamicImportError(error, window.sessionStorage, () =>
      window.location.reload(),
    );
    if (!recovered) throw error;
    return { default: VoiceSecretaryLoadFailure };
  }
}

const VoiceSecretaryComposerControl = lazy(loadVoiceSecretaryComposerControl);

export type { VoiceSecretaryCaptureMode };

export function LazyVoiceSecretaryComposerControl(props: VoiceSecretaryComposerControlProps) {
  const { t } = useTranslation("chat");
  const assistantRow = props.variant === "assistantRow";
  return (
    <Suspense
      fallback={
        <button
          type="button"
          disabled
          aria-label={t("voiceSecretaryInitializing", { defaultValue: "Initializing voice" })}
          className={classNames(
            "glass-btn inline-flex items-center justify-center rounded-lg text-[var(--color-text-secondary)] opacity-60",
            assistantRow ? "h-11 w-24 gap-1.5 text-[11px] font-semibold sm:h-9" : "h-11 w-11",
          )}
        >
          <MicrophoneIcon size={15} aria-hidden="true" />
          {assistantRow ? t("voiceSecretaryInitializingShort", { defaultValue: "Loading" }) : null}
        </button>
      }
    >
      <VoiceSecretaryComposerControl {...props} />
    </Suspense>
  );
}

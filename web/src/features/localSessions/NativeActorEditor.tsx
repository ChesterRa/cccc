import { useTranslation } from "react-i18next";
import { useState } from "react";
import type { EditActorConfigProps } from "../../components/modals/ActorConfigModal";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ModalFrame } from "../../components/modals/ModalFrame";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Textarea } from "../../components/ui/textarea";
export function NativeActorEditor(props: EditActorConfigProps) {
  const { t } = useTranslation("actors");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const busy = saving || props.busy === "actor-update";
  const close = () => {
    if (!busy) props.onCancel();
  };
  const { modalRef } = useModalA11y(props.isOpen, close);
  const save = async () => {
    if (busy || props.actorNotesBusy) return;
    setSaving(true);
    setError("");
    try {
      await props.onSave({
        mode: "custom",
        setVars: {},
        unsetKeys: [],
        clear: false,
        capabilityAutoload: props.savedActor?.capability_autoload || [],
        runtimeMode: props.savedActor?.runtime_mode || "default",
      });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("saveFailed"));
    } finally {
      setSaving(false);
    }
  };
  return (
    <ModalFrame
      isOpen={props.isOpen}
      isDark={props.isDark}
      onClose={close}
      modalRef={modalRef}
      titleId="native-actor-title"
      title={t("editAgent", { actorId: props.actorId })}
      closeAriaLabel={t("common:close")}
      closeDisabled={busy}
      panelClassName="w-full sm:max-w-xl sm:max-h-[80dvh]"
      surface="solid"
      footerActions={
        <Button
          disabled={busy || props.actorNotesBusy}
          aria-busy={busy}
          onClick={() => {
            void save();
          }}
        >
          {t(busy ? "common:saving" : "common:save")}
        </Button>
      }
    >
      <fieldset
        disabled={busy}
        aria-busy={busy || Boolean(props.actorNotesBusy)}
        className="space-y-4 overflow-auto p-4 sm:p-6"
      >
        {error && (
          <p role="alert" className="text-sm text-red-600">
            {error}
          </p>
        )}
        <p className="text-sm text-[var(--color-text-secondary)]">
          {t("nativeSession.configHint")}
        </p>
        <label className="block space-y-2 text-sm">
          <span>{t("displayName")}</span>
          <Input
            value={props.title}
            onChange={(event) => props.onChangeTitle(event.target.value)}
          />
        </label>
        <label className="block space-y-2 text-sm">
          <span>{t("actorNotes")}</span>
          {props.actorNotesBusy && (
            <span role="status" className="block text-xs text-[var(--color-text-secondary)]">
              {t("loadingActorNotes")}
            </span>
          )}
          <Textarea
            value={props.actorNotes}
            onChange={(event) => props.onChangeActorNotes(event.target.value)}
            disabled={props.actorNotesBusy}
          />
        </label>
      </fieldset>
    </ModalFrame>
  );
}

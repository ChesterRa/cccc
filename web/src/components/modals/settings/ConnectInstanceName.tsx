import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiJson } from "../../../services/api/base";
import { inputClass, secondaryButtonClass } from "./types";

export function ConnectInstanceName({
  name,
  origin,
  onSaved,
}: {
  name: string;
  origin: string | null;
  onSaved: (name: string) => void;
}) {
  const { t } = useTranslation("settings");
  const inputId = useId();
  const [draft, setDraft] = useState(name);
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    if (!editing) setDraft(name);
  }, [name, editing]);
  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy || !draft.trim()) return;
    setBusy(true);
    setError("");
    const response = await apiJson<{ display_name: string }>("/api/v1/connect/name", {
      method: "POST",
      body: JSON.stringify({ display_name: draft.trim() }),
      signal: AbortSignal.timeout(35000),
    });
    setBusy(false);
    if (!response.ok) {
      setError(response.error.message);
      return;
    }
    setDraft(response.result.display_name);
    onSaved(response.result.display_name);
    setEditing(false);
  };
  return (
    <form onSubmit={(event) => void save(event)} className="mt-3 space-y-1.5">
      <label
        htmlFor={inputId}
        className="block text-xs font-medium text-[var(--color-text-secondary)]"
      >
        {t("account.connect.instanceName")}
      </label>
      <p className="break-words text-xs text-[var(--color-text-muted)]">
        {t("account.connect.editingInstance", { name })}
        {origin ? <span className="block">{new URL(origin).host}</span> : null}
      </p>
      <div className="mt-1 flex gap-2">
        <input
          id={inputId}
          name="connect-instance-name"
          value={draft}
          maxLength={60}
          required
          disabled={busy}
          onChange={(event) => {
            setEditing(true);
            setDraft(event.target.value);
          }}
          className={`${inputClass()} min-w-0 flex-1`}
        />
        <button
          type="submit"
          disabled={busy || !draft.trim() || draft.trim() === name}
          className={secondaryButtonClass("sm")}
        >
          {t("account.connect.saveName")}
        </button>
      </div>
      <p className="text-xs text-[var(--color-text-muted)]">
        {t("account.connect.instanceNameHint")}
      </p>
      {error ? (
        <p role="alert" className="text-xs text-red-500">
          {error}
        </p>
      ) : null}
    </form>
  );
}

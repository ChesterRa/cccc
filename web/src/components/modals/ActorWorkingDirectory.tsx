import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import type { GroupDoc } from "../../types";
import { SelectCombobox } from "../SelectCombobox";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { CreateGroupDirectoryBrowser } from "./CreateGroupDirectoryBrowser";
import { driveSuggestions } from "./createGroupDirectoryModel";
import { useWorkspaceDirectoryPicker } from "./settings/useWorkspaceDirectoryPicker";

interface ActorWorkingDirectoryProps {
  groupId?: string;
  scopes: NonNullable<GroupDoc["scopes"]>;
  activeScopeKey?: string;
  value: string;
  onChange: (value: string) => void;
  onScopeAttached?: (groupId: string, scope: api.AttachScopeResult["scope"]) => void;
  onBusyChange: (busy: boolean) => void;
  disabled: boolean;
  hint: string;
}

export function ActorWorkingDirectory({
  groupId,
  scopes,
  activeScopeKey,
  value,
  onChange,
  onScopeAttached,
  onBusyChange,
  disabled,
  hint,
}: ActorWorkingDirectoryProps) {
  const { t } = useTranslation("actors");
  const picker = useWorkspaceDirectoryPicker();
  const [pathDraft, setPathDraft] = useState<string | null>(null);
  const [attaching, setAttaching] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const requestSeq = useRef(0);
  useEffect(
    () => () => {
      requestSeq.current += 1;
      onBusyChange(false);
    },
    [onBusyChange],
  );
  const selectedScope = scopes.find((scope) => scope.scope_key === (value || activeScopeKey));
  const path = pathDraft ?? picker.currentDir;
  const choose = (scopeKey: string) => {
    setNotice("");
    onChange(scopeKey);
  };
  const browse = async (directory: string) => {
    const seq = requestSeq.current;
    if ((await picker.fetchDirectory(directory)) && seq === requestSeq.current) {
      setPathDraft(null);
    }
  };
  const cancel = () => {
    requestSeq.current += 1;
    picker.close();
    setAttaching(false);
    onBusyChange(false);
    setError("");
    setNotice(t(attaching ? "workingDirectoryAddStopped" : "workingDirectoryAddCanceled"));
  };
  const attach = async () => {
    if (!groupId || !onScopeAttached || !path.trim() || attaching || disabled) return;
    const seq = ++requestSeq.current;
    setAttaching(true);
    onBusyChange(true);
    setError("");
    setNotice("");
    try {
      const response = await api.attachScope(groupId, path.trim(), { activate: false });
      if (seq !== requestSeq.current) return;
      if (!response.ok) {
        setError(response.error?.message || t("workingDirectoryAddFailed"));
        return;
      }
      const { scope, scope_key: scopeKey, group_id: attachedGroup } = response.result;
      if (attachedGroup !== groupId || !scope?.url || !scopeKey || scope.scope_key !== scopeKey) {
        setError(t("workingDirectoryAddFailed"));
        return;
      }
      const duplicate = scopes.some((item) => item.scope_key === scopeKey);
      onScopeAttached(groupId, scope);
      onChange(scopeKey);
      picker.close();
      setPathDraft(null);
      setNotice(t(duplicate ? "workingDirectoryAlreadyAttached" : "workingDirectoryAdded"));
    } catch (cause) {
      if (seq === requestSeq.current) {
        setError(cause instanceof Error ? cause.message : t("workingDirectoryAddFailed"));
      }
    } finally {
      if (seq === requestSeq.current) {
        setAttaching(false);
        onBusyChange(false);
      }
    }
  };

  return (
    <div className="min-w-0 space-y-2">
      <div className="text-xs font-medium text-[var(--color-text-muted)]">
        {t("workingDirectory")}
      </div>
      {scopes.length > 1 || value ? (
        <SelectCombobox
          ariaLabel={t("workingDirectory")}
          value={value}
          onChange={choose}
          disabled={disabled || attaching}
          placeholder={t("workingDirectoryUnavailable")}
          contentClassName="[&_[role=option]_.truncate]:whitespace-normal [&_[role=option]_.truncate]:break-all"
          className="w-full min-h-[44px] border border-[var(--color-border-secondary)] bg-[var(--color-bg-primary)] px-3 text-sm text-[var(--color-text-primary)]"
          items={[
            { value: "", label: t("followGroupDirectory") },
            ...scopes.map((scope) => ({
              value: scope.scope_key!,
              label: scope.label || scope.url!,
              description: `${t("fixedDirectory")} · ${scope.url}`,
            })),
          ]}
        />
      ) : (
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-sm text-[var(--color-text-secondary)]">
            {t("followGroupDirectory")}
          </span>
          {scopes[0]?.scope_key ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={disabled || attaching}
              onClick={() => choose(scopes[0].scope_key!)}
            >
              {t("fixedDirectory")}
            </Button>
          ) : null}
        </div>
      )}
      <div className="text-xs break-all text-[var(--color-text-muted)]">
        {selectedScope?.url
          ? t("workingDirectoryEffective", { directory: selectedScope.url })
          : t("workingDirectoryUnavailable")}
      </div>
      <div className="text-[10px] text-[var(--color-text-muted)]">{hint}</div>
      {groupId && onScopeAttached && !picker.open ? (
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={disabled}
          onClick={() => {
            setError("");
            setNotice("");
            setPathDraft(null);
            void picker.show(selectedScope?.url || "");
          }}
        >
          {t("workingDirectoryAdd")}
        </Button>
      ) : null}
      {picker.open ? (
        <div className="space-y-2 rounded-xl border p-3 border-[var(--glass-border-subtle)]">
          <p className="text-xs text-[var(--color-text-muted)]">{t("workingDirectoryAddHint")}</p>
          <fieldset disabled={disabled || attaching} className="min-w-0 space-y-2">
            <div className="flex flex-col gap-2 sm:flex-row">
              <Input
                aria-label={t("workingDirectoryPath")}
                value={path}
                onChange={(event) => {
                  setPathDraft(event.target.value);
                  setError("");
                }}
                disabled={picker.busy}
                placeholder={t("workingDirectoryPath")}
                className="min-w-0 flex-1"
              />
              <Button
                type="button"
                variant="secondary"
                disabled={picker.busy || !path.trim()}
                onClick={() => void browse(path.trim())}
              >
                {t("workingDirectoryBrowse")}
              </Button>
            </div>
            <CreateGroupDirectoryBrowser
              dirItems={picker.items}
              currentDir={picker.currentDir}
              parentDir={picker.parentDir}
              driveLocations={driveSuggestions(picker.locations)}
              error={picker.error}
              creatingDirectory={picker.busy}
              onSelect={(directory) => setPathDraft(directory)}
              onFetch={(directory) => void browse(directory)}
              onCreateDirectory={async (parent, name) => {
                const seq = requestSeq.current;
                const created = await picker.createDirectory(parent, name);
                if (created && seq === requestSeq.current) setPathDraft(null);
                return created && seq === requestSeq.current;
              }}
            />
          </fieldset>
          {error ? (
            <p role="alert" className="break-words text-xs text-rose-600 dark:text-rose-400">
              {error}
            </p>
          ) : null}
          <div className="flex flex-wrap justify-end gap-2">
            <Button type="button" variant="secondary" size="sm" onClick={cancel}>
              {t("common:cancel")}
            </Button>
            <Button
              type="button"
              size="sm"
              disabled={disabled || attaching || picker.busy || !path.trim()}
              onClick={() => void attach()}
            >
              {t(attaching ? "workingDirectoryAdding" : "workingDirectoryUse")}
            </Button>
          </div>
        </div>
      ) : null}
      {notice ? (
        <p role="status" className="text-xs text-[var(--color-text-secondary)]">
          {notice}
        </p>
      ) : null}
    </div>
  );
}

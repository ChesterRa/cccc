import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Textarea } from "../../components/ui/textarea";
import { copyTextToClipboard } from "../../utils/copy";
import {
  readDirectInvitation,
  type DirectAddress,
  type DirectListener,
} from "./directConnectionModel";

export function DirectReceiveForm({
  listener,
  addresses,
  inviteMode = false,
  name: initialName,
  busy,
  onSave,
  onCancel,
  onStop,
}: {
  listener: DirectListener | null;
  addresses: DirectAddress[];
  inviteMode?: boolean;
  name: string;
  busy: boolean;
  onSave: (listener: DirectListener, name: string, previous: DirectListener | null) => void;
  onCancel: () => void;
  onStop: () => void;
}) {
  const { t } = useTranslation("layout");
  const id = useId();
  const [savedListener] = useState(listener);
  const [address, setAddress] = useState(listener?.address || addresses[0]?.address || "");
  const [bind, setBind] = useState(listener?.bind || addresses[0]?.bind || "0.0.0.0:8847");
  const [changingAddress, setChangingAddress] = useState(
    !inviteMode || (!listener && !addresses.length),
  );
  const [name, setName] = useState(initialName);
  const [stopping, setStopping] = useState(false);
  const addressInput = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (changingAddress && inviteMode) addressInput.current?.focus();
  }, [changingAddress, inviteMode]);
  return (
    <form
      className="space-y-3"
      onInvalid={(event) => {
        const details = (event.target as HTMLElement).closest("details");
        if (details) details.open = true;
      }}
      onSubmit={(e) => {
        e.preventDefault();
        onSave({ bind: bind.trim(), address: address.trim() }, name.trim(), savedListener);
      }}
    >
      <fieldset disabled={busy} className="space-y-3 min-w-0">
        <p className="text-[var(--color-text-secondary)]">
          {t(inviteMode ? "direct.inviteHint" : "direct.setupHint")}
        </p>
        <div className="space-y-2">
          <div className="flex items-center justify-between gap-3">
            <label
              className="block font-medium"
              htmlFor={changingAddress ? `${id}-address` : undefined}
            >
              {t("direct.address")}
            </label>
            {!changingAddress && (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                onClick={() => setChangingAddress(true)}
              >
                {t("direct.changeAddress")}
              </Button>
            )}
          </div>
          {changingAddress ? (
            <>
              {addresses.length > 0 && (
                <div className="space-y-1">
                  <label
                    className="block text-xs text-[var(--color-text-secondary)]"
                    htmlFor={`${id}-candidates`}
                  >
                    {t("direct.localAddresses")}
                  </label>
                  <select
                    id={`${id}-candidates`}
                    className="glass-input w-full min-h-[44px] rounded-xl px-3 text-sm"
                    value=""
                    onChange={(e) => {
                      const selected = addresses.find((a) => a.address === e.target.value);
                      if (selected) {
                        setAddress(selected.address);
                        setBind(selected.bind);
                      }
                    }}
                  >
                    <option value="" disabled>
                      {t("direct.selectAddress")}
                    </option>
                    {addresses.map((a) => (
                      <option key={a.address} value={a.address}>
                        {a.address} · {a.interface}
                      </option>
                    ))}
                  </select>
                </div>
              )}
              <Input
                ref={addressInput}
                id={`${id}-address`}
                required
                maxLength={256}
                value={address}
                onChange={(e) => setAddress(e.target.value)}
                placeholder="192.168.1.10:8847"
                autoComplete="off"
                spellCheck={false}
                aria-describedby={`${id}-address-hint`}
              />
              <p id={`${id}-address-hint`} className="text-xs text-[var(--color-text-secondary)]">
                {t(addresses.length || savedListener ? "direct.addressHint" : "direct.noAddress")}
              </p>
            </>
          ) : (
            <p className="break-all font-mono">{address}</p>
          )}
          {inviteMode && (
            <p className="text-xs text-[var(--color-text-secondary)]">
              {t(savedListener ? "direct.savedAddress" : "direct.suggestedAddress")}
            </p>
          )}
        </div>
        {inviteMode && !savedListener && (
          <p className="text-xs text-[var(--color-text-secondary)]">{t("direct.createEffect")}</p>
        )}
        <details className="space-y-3">
          <summary className="cursor-pointer">{t("direct.advanced")}</summary>
          <div className="space-y-2">
            <p className="text-xs text-[var(--color-text-secondary)]">
              {t("direct.listenEffect", { bind })}
            </p>
            <label className="block" htmlFor={`${id}-bind`}>
              {t("direct.bind")}
            </label>
            <Input
              id={`${id}-bind`}
              required
              value={bind}
              onChange={(e) => setBind(e.target.value)}
            />
            <p className="text-xs text-[var(--color-text-secondary)]">{t("direct.bindHint")}</p>
            <label className="block" htmlFor={`${id}-name`}>
              {t("direct.name")}
            </label>
            <Input
              id={`${id}-name`}
              maxLength={60}
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
        </details>
        <div className="flex flex-wrap gap-2">
          <Button type="submit" disabled={busy || !address.trim() || !bind.trim()}>
            {t(
              busy
                ? inviteMode
                  ? "direct.creating"
                  : "direct.saving"
                : inviteMode
                  ? "direct.create"
                  : "direct.saveChanges",
            )}
          </Button>
          <Button type="button" variant="ghost" disabled={busy} onClick={onCancel}>
            {t("direct.cancel")}
          </Button>
        </div>
        {!inviteMode && listener && (
          <div className="border-t border-[var(--glass-border-subtle)] pt-3">
            {stopping ? (
              <div className="space-y-2">
                <p>{t("direct.stopHint")}</p>
                <div className="flex flex-wrap gap-2">
                  <Button type="button" variant="outline" disabled={busy} onClick={onStop}>
                    {t("direct.stopConfirm")}
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    disabled={busy}
                    onClick={() => setStopping(false)}
                  >
                    {t("direct.cancel")}
                  </Button>
                </div>
              </div>
            ) : (
              <Button
                type="button"
                variant="ghost"
                disabled={busy}
                onClick={() => setStopping(true)}
              >
                {t("direct.stop")}
              </Button>
            )}
          </div>
        )}
      </fieldset>
    </form>
  );
}

export function DirectJoinForm({
  groupTitle,
  incoming,
  setIncoming,
  busy,
  onJoin,
}: {
  groupTitle: string;
  incoming: string;
  setIncoming: (value: string) => void;
  busy: boolean;
  onJoin: () => void;
}) {
  const { t, i18n } = useTranslation("layout");
  const id = useId();
  const preview = readDirectInvitation(incoming);
  const expired = preview && Date.parse(preview.expires_at) <= Date.now();
  return (
    <form
      className="space-y-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (preview && !expired) onJoin();
      }}
    >
      <p className="text-[var(--color-text-secondary)]">{t("direct.joinHint")}</p>
      <label htmlFor={id} className="block font-medium">
        {t("direct.pasteInvite")}
      </label>
      <Textarea
        id={id}
        rows={3}
        maxLength={8192}
        value={incoming}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => setIncoming(e.target.value)}
        placeholder="cccc-direct:…"
      />
      {incoming.trim() && !preview && <p role="alert">{t("direct.invalidInvitation")}</p>}
      {preview && (
        <div className="space-y-2 border-l-2 border-[var(--color-border-focus)] pl-3">
          <p className="text-[var(--color-text-secondary)]">{t("direct.reviewPair")}</p>
          <p className="break-words font-medium">
            {groupTitle} ↔ {preview.host.name} · {preview.host.title}
          </p>
          <p className="break-words text-xs text-[var(--color-text-secondary)]">
            {preview.address}
          </p>
          <p className="text-xs" role={expired ? "alert" : undefined}>
            {expired
              ? t("direct.expiredHint")
              : t("direct.expiresAt", {
                  time: new Date(preview.expires_at).toLocaleString(i18n.language),
                })}
          </p>
          <details className="text-xs text-[var(--color-text-secondary)]">
            <summary className="cursor-pointer">{t("direct.identity")}</summary>
            <p className="break-all py-1">{preview.host.instance_id}</p>
          </details>
        </div>
      )}
      <Button type="submit" disabled={busy || !preview || !!expired}>
        {t(busy ? "direct.requesting" : "direct.request")}
      </Button>
    </form>
  );
}

export function DirectInvitationShare({ text }: { text: string }) {
  const { t, i18n } = useTranslation("layout");
  const [copy, setCopy] = useState<"idle" | "copied" | "failed">("idle");
  const preview = readDirectInvitation(text);
  if (!preview) return null;
  if (Date.parse(preview.expires_at) <= Date.now()) return <p>{t("direct.expiredHint")}</p>;
  return (
    <div className="space-y-2">
      <p>{t("direct.shareHint")}</p>
      <p className="text-xs text-[var(--color-text-secondary)]">
        {t("direct.expiresAt", {
          time: new Date(preview.expires_at).toLocaleString(i18n.language),
        })}
      </p>
      <Textarea
        aria-label={t("direct.invitation")}
        rows={2}
        readOnly
        value={text}
        onFocus={(e) => e.target.select()}
      />
      <Button
        variant="outline"
        size="sm"
        onClick={async () => {
          const focus = document.activeElement as HTMLElement | null;
          setCopy((await copyTextToClipboard(text)) ? "copied" : "failed");
          if (focus?.isConnected) focus.focus();
        }}
      >
        {t(copy === "copied" ? "direct.copied" : "direct.copy")}
      </Button>
      {copy === "failed" && <p role="alert">{t("direct.copyFailed")}</p>}
      <p className="text-xs text-[var(--color-text-secondary)]">{t("direct.keepInvitation")}</p>
    </div>
  );
}

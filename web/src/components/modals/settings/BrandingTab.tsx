import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";
import { useBrandingStore } from "../../../stores";
import { resolveThemeAwareFaviconUrl, resolveThemeAwareLogoUrl } from "../../../utils/branding";
import {
  inputClass,
  labelClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspaceShellClass,
} from "./types";

interface BrandingTabProps {
  isDark: boolean;
  isActive?: boolean;
}

type AssetKind = "logo_icon" | "favicon";

export function BrandingTab({ isDark, isActive = true }: BrandingTabProps) {
  const { t } = useTranslation("settings");
  const branding = useBrandingStore((s) => s.branding);
  const setBranding = useBrandingStore((s) => s.setBranding);
  const refreshBranding = useBrandingStore((s) => s.refreshBranding);

  const [productName, setProductName] = useState(branding.product_name);
  const [busy, setBusy] = useState<"" | "save" | AssetKind>("");
  const [error, setError] = useState("");
  const [hint, setHint] = useState("");

  const logoInputRef = useRef<HTMLInputElement | null>(null);
  const faviconInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    setProductName(branding.product_name);
  }, [branding.product_name]);

  useEffect(() => {
    if (!isActive) return;
    void refreshBranding();
  }, [isActive, refreshBranding]);

  const previewName = useMemo(
    () => String(productName || "").trim() || branding.product_name,
    [branding.product_name, productName],
  );
  const logoSrc = useMemo(
    () => resolveThemeAwareLogoUrl(branding.logo_icon_url, isDark),
    [branding.logo_icon_url, isDark],
  );
  const faviconSrc = useMemo(
    () => resolveThemeAwareFaviconUrl(branding.favicon_url || branding.logo_icon_url, isDark),
    [branding.favicon_url, branding.logo_icon_url, isDark],
  );

  const pushHint = (value: string) => {
    setHint(value);
    window.setTimeout(() => setHint(""), 1800);
  };

  const handleSaveName = async () => {
    setBusy("save");
    setError("");
    try {
      const resp = await api.updateWebBranding({ productName });
      if (!resp.ok) {
        setError(resp.error?.message || t("branding.saveFailed"));
        return;
      }
      setBranding(resp.result?.branding);
      pushHint(t("branding.saved"));
    } catch {
      setError(t("branding.saveFailed"));
    } finally {
      setBusy("");
    }
  };

  const handleUpload = async (assetKind: AssetKind, file: File | null) => {
    if (!file) return;
    setBusy(assetKind);
    setError("");
    try {
      const resp = await api.uploadWebBrandingAsset(assetKind, file);
      if (!resp.ok) {
        setError(resp.error?.message || t("branding.uploadFailed"));
        return;
      }
      setBranding(resp.result?.branding);
      pushHint(assetKind === "logo_icon" ? t("branding.logoSaved") : t("branding.faviconSaved"));
    } catch {
      setError(t("branding.uploadFailed"));
    } finally {
      setBusy("");
      if (assetKind === "logo_icon" && logoInputRef.current) logoInputRef.current.value = "";
      if (assetKind === "favicon" && faviconInputRef.current) faviconInputRef.current.value = "";
    }
  };

  const handleClear = async (assetKind: AssetKind) => {
    setBusy(assetKind);
    setError("");
    try {
      const resp = await api.clearWebBrandingAsset(assetKind);
      if (!resp.ok) {
        setError(resp.error?.message || t("branding.saveFailed"));
        return;
      }
      setBranding(resp.result?.branding);
      pushHint(assetKind === "logo_icon" ? t("branding.logoReset") : t("branding.faviconReset"));
    } catch {
      setError(t("branding.saveFailed"));
    } finally {
      setBusy("");
    }
  };

  return (
    <section className={settingsWorkspaceShellClass(isDark)}>
      <div className={settingsWorkspaceHeaderClass(isDark)}>
        <div className="min-w-0">
          <h3 className="text-base font-semibold">{t("branding.title")}</h3>
          <p className="mt-1 text-sm text-[var(--color-text-secondary)]">
            {t("branding.description")}
          </p>
        </div>
      </div>
      <div className={`${settingsWorkspaceBodyClass} max-w-3xl`}>
        <div
          aria-label={t("branding.preview")}
          className="flex min-w-0 items-center gap-3 rounded-lg bg-[var(--color-bg-secondary)] px-4 py-3"
        >
          <img src={logoSrc} alt="" className="h-9 w-auto max-w-24 shrink-0 object-contain" />
          <span className="min-w-0 truncate text-lg font-semibold">{previewName}</span>
        </div>
        <form
          className="border-b border-[var(--glass-border-subtle)] py-4"
          onSubmit={(event) => {
            event.preventDefault();
            if (!busy) void handleSaveName();
          }}
        >
          <label htmlFor="branding-product-name" className={labelClass()}>
            {t("branding.productNameTitle")}
          </label>
          <div className="flex flex-wrap items-start gap-2">
            <input
              id="branding-product-name"
              value={productName}
              onChange={(event) => setProductName(event.target.value)}
              maxLength={80}
              className={`${inputClass()} min-w-0 flex-1 basis-48`}
              placeholder={t("branding.productNamePlaceholder")}
              aria-describedby="branding-name-hint"
            />
            <button
              type="submit"
              disabled={busy !== "" || productName === branding.product_name}
              className={primaryButtonClass()}
            >
              {busy === "save" ? t("common:saving") : t("branding.saveName")}
            </button>
          </div>
          <p
            id="branding-name-hint"
            className="mt-2 text-xs leading-5 text-[var(--color-text-secondary)]"
          >
            {t("branding.productNameHint")}
          </p>
        </form>
        {(["logo_icon", "favicon"] as const).map((kind) => {
          const logo = kind === "logo_icon";
          const inputRef = logo ? logoInputRef : faviconInputRef;
          return (
            <div
              key={kind}
              className="flex min-w-0 items-start gap-4 border-b border-[var(--glass-border-subtle)] py-4 last:border-b-0"
            >
              <div className="flex h-16 w-16 shrink-0 items-center justify-center rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] p-2">
                <img
                  src={logo ? logoSrc : faviconSrc}
                  alt=""
                  className={logo ? "max-h-10 max-w-full object-contain" : "h-8 w-8 object-contain"}
                />
              </div>
              <div className="min-w-0 flex-1">
                <h4 className="text-sm font-semibold">
                  {t(logo ? "branding.logoTitle" : "branding.faviconTitle")}
                </h4>
                <p className="mt-1 text-xs leading-5 text-[var(--color-text-secondary)]">
                  {t(logo ? "branding.logoHint" : "branding.faviconHint")}
                </p>
                <input
                  ref={inputRef}
                  type="file"
                  className="hidden"
                  accept={
                    logo
                      ? ".svg,.png,.jpg,.jpeg,.webp,.gif,.avif,.ico,image/*"
                      : ".svg,.png,.ico,image/svg+xml,image/png,image/x-icon,image/vnd.microsoft.icon"
                  }
                  onChange={(event) => void handleUpload(kind, event.target.files?.[0] || null)}
                />
                <div className="mt-3 flex flex-wrap gap-2">
                  <button
                    type="button"
                    disabled={busy !== ""}
                    className={secondaryButtonClass("sm")}
                    onClick={() => inputRef.current?.click()}
                  >
                    {busy === kind
                      ? t("branding.uploading")
                      : t(logo ? "branding.uploadLogo" : "branding.uploadFavicon")}
                  </button>
                  <button
                    type="button"
                    className={secondaryButtonClass("sm")}
                    disabled={
                      busy !== "" ||
                      !(logo ? branding.has_custom_logo_icon : branding.has_custom_favicon)
                    }
                    onClick={() => void handleClear(kind)}
                  >
                    {t(logo ? "branding.useDefault" : "branding.followLogo")}
                  </button>
                </div>
              </div>
            </div>
          );
        })}
        <p className="text-xs text-[var(--color-text-secondary)]">
          {t("branding.uploadAppliesImmediately")}
        </p>
        {hint && (
          <p role="status" className="text-sm text-[var(--color-accent-success)]">
            {hint}
          </p>
        )}
        {error && (
          <p role="alert" className="text-sm text-rose-600 dark:text-rose-400">
            {error}
          </p>
        )}
      </div>
    </section>
  );
}

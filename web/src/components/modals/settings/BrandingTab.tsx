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
  settingsWorkspaceActionBarClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  settingsWorkspaceSoftPanelClass,
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
  const [pendingLogoName, setPendingLogoName] = useState("");
  const [pendingFaviconName, setPendingFaviconName] = useState("");

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
      if (assetKind === "logo_icon") setPendingLogoName("");
      if (assetKind === "favicon") setPendingFaviconName("");
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
    <div className="space-y-5">
      <div className={settingsWorkspaceShellClass(isDark)}>
        <div className={settingsWorkspaceHeaderClass(isDark)}>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
              {t("branding.title")}
            </h3>
            <p className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("branding.description")}
            </p>
          </div>
        </div>

        <div className={settingsWorkspaceBodyClass}>
          <div className={settingsWorkspacePanelClass(isDark)}>
            <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
              {t("branding.preview")}
            </div>
            <div className={`mt-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
              <div className="flex items-center gap-3 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3">
                <div className="flex h-12 min-w-[48px] max-w-[180px] items-center justify-center overflow-hidden rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3 shadow-sm">
                  <img
                    src={logoSrc}
                    alt={`${branding.product_name} logo`}
                    className="max-h-7 w-auto max-w-full object-contain"
                  />
                </div>
                <div className="min-w-0">
                  <div className="truncate text-base font-semibold text-[var(--color-text-primary)]">
                    {previewName}
                  </div>
                  <div className="mt-1 text-xs text-[var(--color-text-muted)]">
                    {branding.has_custom_favicon
                      ? t("branding.previewFaviconCustom")
                      : t("branding.previewFaviconFollow")}
                  </div>
                </div>
              </div>
            </div>
          </div>

          <div className={settingsWorkspacePanelClass(isDark)}>
            <div className="text-sm font-semibold text-[var(--color-text-primary)]">
              {t("branding.productNameTitle")}
            </div>
            <div className="mt-1 text-xs text-[var(--color-text-muted)]">
              {t("branding.productNameHint")}
            </div>
            <div className="mt-4">
              <label className={labelClass(isDark)}>{t("branding.productNameLabel")}</label>
              <input
                value={productName}
                onChange={(e) => setProductName(e.target.value)}
                maxLength={80}
                className={inputClass(isDark)}
                placeholder={t("branding.productNamePlaceholder")}
              />
            </div>
            <div className="mt-4">
              <button
                type="button"
                onClick={() => void handleSaveName()}
                disabled={busy !== ""}
                className={primaryButtonClass(busy !== "")}
              >
                {busy === "save" ? t("common:saving") : t("branding.saveName")}
              </button>
            </div>
          </div>

          <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
            <div className={settingsWorkspacePanelClass(isDark)}>
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                {t("branding.logoTitle")}
              </div>
              <div className="mt-1 text-xs text-[var(--color-text-muted)]">
                {t("branding.logoHint")}
              </div>
              <div className={`mt-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                <div className="flex h-20 items-center justify-center overflow-hidden rounded-2xl border border-dashed border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4">
                  <img
                    src={logoSrc}
                    alt={`${branding.product_name} logo`}
                    className="max-h-12 w-auto max-w-full object-contain"
                  />
                </div>
              </div>
              <input
                ref={logoInputRef}
                type="file"
                accept=".svg,.png,.jpg,.jpeg,.webp,.gif,.avif,.ico,image/*"
                className="hidden"
                onChange={(e) => {
                  const nextFile = e.target.files?.[0] || null;
                  setPendingLogoName(nextFile?.name || "");
                  void handleUpload("logo_icon", nextFile);
                }}
              />
              <div className={`mt-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                <div className="flex flex-wrap items-center gap-3">
                  <button
                    type="button"
                    className={secondaryButtonClass()}
                    disabled={busy !== ""}
                    onClick={() => logoInputRef.current?.click()}
                  >
                    {busy === "logo_icon" ? t("branding.uploading") : t("branding.uploadLogo")}
                  </button>
                  <div
                    className={`min-w-0 text-sm ${pendingLogoName ? "text-[var(--color-text-primary)]" : "text-[var(--color-text-muted)]"}`}
                  >
                    <span className="block truncate">
                      {pendingLogoName || t("common:noFileChosen", "No file chosen")}
                    </span>
                  </div>
                </div>
              </div>
              <div className="mt-4 flex flex-wrap gap-2">
                <button
                  type="button"
                  className={secondaryButtonClass()}
                  disabled={busy !== "" || !branding.has_custom_logo_icon}
                  onClick={() => void handleClear("logo_icon")}
                >
                  {t("branding.useDefault")}
                </button>
              </div>
            </div>

            <div className={settingsWorkspacePanelClass(isDark)}>
              <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                {t("branding.faviconTitle")}
              </div>
              <div className="mt-1 text-xs text-[var(--color-text-muted)]">
                {t("branding.faviconHint")}
              </div>
              <div className={`mt-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                <div className="flex h-20 items-center justify-center rounded-2xl border border-dashed border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)]">
                  <img
                    src={faviconSrc}
                    alt={`${branding.product_name} favicon`}
                    className="h-8 w-8 object-contain"
                  />
                </div>
              </div>
              <input
                ref={faviconInputRef}
                type="file"
                accept=".svg,.png,.ico,image/svg+xml,image/png,image/x-icon,image/vnd.microsoft.icon"
                className="hidden"
                onChange={(e) => {
                  const nextFile = e.target.files?.[0] || null;
                  setPendingFaviconName(nextFile?.name || "");
                  void handleUpload("favicon", nextFile);
                }}
              />
              <div className={`mt-4 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                <div className="flex flex-wrap items-center gap-3">
                  <button
                    type="button"
                    className={secondaryButtonClass()}
                    disabled={busy !== ""}
                    onClick={() => faviconInputRef.current?.click()}
                  >
                    {busy === "favicon" ? t("branding.uploading") : t("branding.uploadFavicon")}
                  </button>
                  <div
                    className={`min-w-0 text-sm ${pendingFaviconName ? "text-[var(--color-text-primary)]" : "text-[var(--color-text-muted)]"}`}
                  >
                    <span className="block truncate">
                      {pendingFaviconName || t("common:noFileChosen", "No file chosen")}
                    </span>
                  </div>
                </div>
              </div>
              <div className="mt-4 flex flex-wrap gap-2">
                <button
                  type="button"
                  className={secondaryButtonClass()}
                  disabled={busy !== "" || !branding.has_custom_favicon}
                  onClick={() => void handleClear("favicon")}
                >
                  {t("branding.followLogo")}
                </button>
              </div>
            </div>
          </div>
        </div>

        <div className={settingsWorkspaceActionBarClass(isDark)}>
          {hint ? (
            <div className="text-xs text-emerald-600 dark:text-emerald-400">{hint}</div>
          ) : null}
          {error ? <div className="text-xs text-rose-600 dark:text-rose-400">{error}</div> : null}
        </div>
      </div>
    </div>
  );
}

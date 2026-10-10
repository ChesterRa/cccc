import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import * as api from "../../../services/api";
import { useBrandingStore } from "../../../stores";
import { resolveThemeAwareFaviconUrl, resolveThemeAwareLogoUrl } from "../../../utils/branding";
import { inputClass, secondaryButtonClass } from "./types";
import { SettingsCard, SettingsPage, SettingsRow, SettingsSaveBar } from "./SettingsKit";

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

  const saveName = () => {
    if (!busy) void handleSaveName();
  };

  return (
    <SettingsPage
      title={t("branding.title")}
      description={
        <>
          {t("branding.description")} {t("branding.uploadAppliesImmediately")}
        </>
      }
      footer={
        <SettingsSaveBar
          label={t("branding.saveName")}
          busy={busy === "save"}
          disabled={busy !== "" || productName === branding.product_name}
          onSave={saveName}
        />
      }
    >
      {hint ? (
        <p role="status" className="text-sm text-[var(--color-accent-success)]">
          {hint}
        </p>
      ) : null}
      {error ? (
        <p role="alert" className="text-sm text-rose-600 dark:text-rose-400">
          {error}
        </p>
      ) : null}

      <SettingsCard title={t("branding.preview")}>
        <div className="px-5 py-4">
          <div
            aria-label={t("branding.preview")}
            className="flex min-w-0 items-center gap-3 rounded-[10px] bg-[var(--color-bg-secondary)] px-4 py-3"
          >
            <img src={logoSrc} alt="" className="h-9 w-auto max-w-24 shrink-0 object-contain" />
            <span className="min-w-0 truncate text-lg font-semibold">{previewName}</span>
          </div>
        </div>
      </SettingsCard>

      <SettingsCard>
        <SettingsRow
          label={t("branding.productNameTitle")}
          help={t("branding.productNameHint")}
          htmlFor="branding-product-name"
        >
          <form
            className="w-full max-w-[360px]"
            onSubmit={(event) => {
              event.preventDefault();
              saveName();
            }}
          >
            <input
              id="branding-product-name"
              value={productName}
              onChange={(event) => setProductName(event.target.value)}
              maxLength={80}
              className={inputClass()}
              placeholder={t("branding.productNamePlaceholder")}
            />
          </form>
        </SettingsRow>
        {(["logo_icon", "favicon"] as const).map((kind) => {
          const logo = kind === "logo_icon";
          const inputRef = logo ? logoInputRef : faviconInputRef;
          return (
            <SettingsRow
              key={kind}
              label={t(logo ? "branding.logoTitle" : "branding.faviconTitle")}
              help={t(logo ? "branding.logoHint" : "branding.faviconHint")}
            >
              <span className="flex h-12 w-12 shrink-0 items-center justify-center rounded-[10px] border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] p-1.5">
                <img
                  src={logo ? logoSrc : faviconSrc}
                  alt=""
                  className={logo ? "max-h-8 max-w-full object-contain" : "h-6 w-6 object-contain"}
                />
              </span>
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
            </SettingsRow>
          );
        })}
      </SettingsCard>
    </SettingsPage>
  );
}

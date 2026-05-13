import { memo, useMemo, useState } from "react";
import { classNames } from "../utils/classNames";
import { withAuthToken } from "../services/api/base";
import { getRuntimeLogoSrc } from "../utils/runtimeLogos";
import { ClaudeLogo } from "./runtimeLogos/ClaudeLogo";
export type ActorAvatarProps = {
  avatarUrl?: string | null;
  previewUrl?: string | null;
  runtime?: string | null;
  title?: string | null;
  isUser?: boolean;
  isDark: boolean;
  accentRingClassName?: string | null;
  sizeClassName?: string;
  textClassName?: string;
  className?: string;
};

export const ActorAvatar = memo(function ActorAvatar({
  avatarUrl,
  previewUrl,
  runtime,
  title,
  isUser = false,
  isDark,
  accentRingClassName,
  sizeClassName = "h-8 w-8",
  textClassName = "text-xs",
  className,
}: ActorAvatarProps) {
  const previewSrc = useMemo(() => {
    if (isUser) return null;
    const raw = String(previewUrl || "").trim();
    return raw || null;
  }, [previewUrl, isUser]);

  const customAvatarSrc = useMemo(() => {
    if (isUser) return null;
    const raw = String(avatarUrl || "").trim();
    if (!raw) return null;
    return raw.includes("token=") ? raw : withAuthToken(raw);
  }, [avatarUrl, isUser]);
  const [failedCustomAvatarSrc, setFailedCustomAvatarSrc] = useState<string | null>(null);
  const customAvatarFailed = !!customAvatarSrc && failedCustomAvatarSrc === customAvatarSrc;

  const usesInlineClaudeLogo = !isUser && String(runtime || "").trim().toLowerCase() === "claude";

  const logoSrc = useMemo(() => {
    if (isUser || usesInlineClaudeLogo) return null;
    return getRuntimeLogoSrc(runtime);
  }, [isUser, runtime, usesInlineClaudeLogo]);

  const fallbackText = isUser ? "U" : (String(title || "").trim() || "?")[0].toUpperCase();

  return (
    <div
      className={classNames(
        "flex flex-shrink-0 items-center justify-center overflow-hidden rounded-full font-bold shadow-sm",
        sizeClassName,
        textClassName,
        isUser
          ? "bg-[linear-gradient(135deg,rgb(245,245,245)_0%,rgb(232,234,236)_100%)] text-[rgb(35,36,37)] border border-black/6"
          : isDark
            ? "bg-slate-700 text-slate-200"
            : "border border-gray-200 bg-white text-gray-700",
        !isUser && accentRingClassName ? `ring-1 ring-inset ${accentRingClassName}` : "",
        className,
      )}
    >
      {previewSrc ? (
        <img src={previewSrc} alt="" className="h-full w-full object-contain" />
      ) : customAvatarSrc && !customAvatarFailed ? (
        <img
          src={customAvatarSrc}
          alt=""
          className="h-full w-full object-contain"
          onError={() => setFailedCustomAvatarSrc(customAvatarSrc)}
        />
      ) : usesInlineClaudeLogo ? (
        <ClaudeLogo className="h-3/5 w-3/5" />
      ) : logoSrc ? (
        <img src={logoSrc} alt="" className="h-full w-full object-cover" />
      ) : (
        fallbackText
      )}
    </div>
  );
});

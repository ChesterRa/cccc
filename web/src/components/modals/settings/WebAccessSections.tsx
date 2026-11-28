import { useEffect, useState, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { ActorConfigTabs } from "../ActorConfigTabs";

/** Keep Web login management apart from reachability, preserving both sets of drafts. */
export function WebAccessSections({
  isActive,
  focusReach,
  reachSectionRef,
  tokens,
  children,
}: {
  isActive: boolean;
  focusReach: boolean;
  reachSectionRef: RefObject<HTMLElement | null>;
  tokens: ReactNode;
  children: ReactNode;
}) {
  const { t } = useTranslation("settings");
  const [section, setSection] = useState("overview");

  useEffect(() => {
    if (isActive && focusReach) setSection("overview");
  }, [focusReach, isActive]);

  useEffect(() => {
    if (!isActive || !focusReach || section !== "overview") return;
    let innerFrame = 0;
    const outerFrame = window.requestAnimationFrame(() => {
      innerFrame = window.requestAnimationFrame(() => {
        reachSectionRef.current?.scrollIntoView({ block: "start", behavior: "smooth" });
      });
    });
    return () => {
      window.cancelAnimationFrame(outerFrame);
      if (innerFrame) window.cancelAnimationFrame(innerFrame);
    };
  }, [focusReach, isActive, reachSectionRef, section]);

  return (
    <ActorConfigTabs
      ariaLabel={t("webAccess.title")}
      activeId={section}
      onChange={setSection}
      tabs={[
        {
          id: "overview",
          label: t("webAccess.overviewTab"),
          panel: <div className="space-y-5">{children}</div>,
        },
        { id: "tokens", label: t("webAccess.accessControlTitle"), panel: tokens },
      ]}
    />
  );
}

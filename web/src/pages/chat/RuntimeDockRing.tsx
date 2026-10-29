import { useSyncExternalStore } from "react";
import { classNames } from "../../utils/classNames";
import "./runtimeDockRing.css";

const subscribeVisibility = (notify: () => void) => {
  document.addEventListener("visibilitychange", notify);
  return () => document.removeEventListener("visibilitychange", notify);
};
const isVisible = () => document.visibilityState !== "hidden";

export function RuntimeDockRing({
  size,
  active,
  colorClassName,
  gradientId,
}: {
  size: number;
  active: boolean;
  colorClassName: string;
  gradientId: string;
}) {
  const visible = useSyncExternalStore(subscribeVisibility, isVisible, () => false);
  return (
    <span
      aria-hidden="true"
      className={classNames(
        "runtime-dock-ring-layer pointer-events-none absolute",
        active && "runtime-dock-ring-layer--active",
      )}
      data-animating={active && visible}
      style={{ width: size, height: size }}
    >
      <svg
        aria-hidden="true"
        viewBox="0 0 44 44"
        width={size}
        height={size}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        className={classNames(
          "runtime-dock-ring block transition-colors duration-200",
          colorClassName,
        )}
      >
        <circle cx="22" cy="22" r="21" strokeWidth={active ? 2 : 1.5} opacity={active ? 0.12 : 1} />
        {active ? (
          <>
            <defs>
              <linearGradient
                id={gradientId}
                gradientUnits="userSpaceOnUse"
                x1="43"
                y1="22"
                x2="11.5"
                y2="40.2"
              >
                <stop stopColor="currentColor" stopOpacity="0" />
                <stop offset="0.55" stopColor="currentColor" stopOpacity="0.85" />
                <stop offset="0.85" stopColor="currentColor" />
                <stop offset="1" stopColor="#FE8FB5" />
              </linearGradient>
            </defs>
            <circle
              className="runtime-dock-ring__activity"
              cx="22"
              cy="22"
              r="21"
              stroke={`url(#${gradientId})`}
              strokeDasharray="44 88"
              strokeLinecap="round"
            />
          </>
        ) : null}
      </svg>
    </span>
  );
}

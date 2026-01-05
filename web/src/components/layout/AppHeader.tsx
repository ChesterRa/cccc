import { Actor, GroupDoc, Theme } from "../../types";
import { getGroupStatus, getGroupStatusLight } from "../../utils/groupStatus";
import { classNames } from "../../utils/classNames";
import { ThemeToggleCompact } from "../ThemeToggle";
import { 
  ClipboardIcon, 
  RocketIcon, 
  PlayIcon, 
  PauseIcon, 
  StopIcon, 
  SettingsIcon, 
  EditIcon,
  MoreIcon,
  MenuIcon 
} from "../Icons";

export interface AppHeaderProps {
  isDark: boolean;
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  selectedGroupId: string;
  groupDoc: GroupDoc | null;
  selectedGroupRunning: boolean;
  actors: Actor[];
  busy: string;
  errorMsg: string;
  onDismissError: () => void;
  onOpenSidebar: () => void;
  onOpenGroupEdit: () => void;
  onOpenContext: () => void;
  onStartGroup: () => void;
  onStopGroup: () => void;
  onSetGroupState: (state: "active" | "paused" | "idle") => void | Promise<void>;
  onOpenSettings: () => void;
  onOpenMobileMenu: () => void;
}

export function AppHeader({
  isDark,
  theme,
  onThemeChange,
  selectedGroupId,
  groupDoc,
  selectedGroupRunning,
  actors,
  busy,
  errorMsg,
  onDismissError,
  onOpenSidebar,
  onOpenGroupEdit,
  onOpenContext,
  onStartGroup,
  onStopGroup,
  onSetGroupState,
  onOpenSettings,
  onOpenMobileMenu,
}: AppHeaderProps) {
  return (
    <header
      className="flex-shrink-0 z-20 px-4 h-14 flex items-center justify-between gap-3 glass-header"
    >
      <div className="flex items-center gap-3 min-w-0">
        <button
          className={classNames(
            "md:hidden p-2 -ml-2 rounded-xl transition-all glass-btn",
            isDark ? "text-slate-400 hover:text-white" : "text-gray-500 hover:text-gray-900"
          )}
          onClick={onOpenSidebar}
          aria-label="Open sidebar"
        >
          <MenuIcon size={18} />
        </button>

        <div className="min-w-0 flex flex-col">
          <div className="flex items-center gap-2">
            <h1 className={`text-sm font-semibold truncate ${isDark ? "text-slate-100" : "text-gray-900"}`}>
              {groupDoc?.title || (selectedGroupId ? selectedGroupId : "Select a group")}
            </h1>
            {selectedGroupId &&
              groupDoc &&
              (() => {
                const status = isDark ? getGroupStatus(selectedGroupRunning, groupDoc.state) : getGroupStatusLight(selectedGroupRunning, groupDoc.state);
                return (
                  <span
                    className={`w-2 h-2 rounded-full ${status.colorClass.replace("text-", "bg-").split(" ")[0]}`}
                    title={status.label}
                  />
                );
              })()}
          </div>
        </div>

        {selectedGroupId && (
          <button
            className={classNames(
              "hidden sm:inline-flex items-center justify-center gap-1 text-xs px-2.5 py-1.5 rounded-xl transition-all glass-btn",
              isDark ? "text-slate-200" : "text-gray-700"
            )}
            onClick={onOpenGroupEdit}
            title="Edit group"
            aria-label="Edit group"
          >
            <EditIcon size={14} />
          </button>
        )}
      </div>

      {/* Right Actions */}
      <div className="flex items-center gap-1">
        {/* Desktop Actions - subtle */}
        <div className="hidden sm:flex items-center gap-1.5 mr-2">
          <button
            onClick={onOpenContext}
            disabled={!selectedGroupId}
            className={classNames(
              "p-2 rounded-xl transition-all glass-btn",
              isDark ? "text-slate-400 hover:text-white" : "text-gray-400 hover:text-gray-900"
            )}
            title="Context (Clipboard)"
          >
            <span className="sr-only">Context</span>
            <ClipboardIcon size={18} />
          </button>

          <div className={`w-px h-4 mx-1 ${isDark ? "bg-white/10" : "bg-black/10"}`} />

          <button
            onClick={onStartGroup}
            disabled={!selectedGroupId || busy === "group-start" || actors.length === 0}
            className={classNames(
              "p-2 rounded-xl transition-all",
              isDark 
                ? "text-emerald-400 hover:bg-emerald-500/15 glass-btn" 
                : "text-emerald-600 hover:bg-emerald-50/80 glass-btn"
            )}
            title="Launch All Agents"
          >
            <span className="sr-only">Launch</span>
            <RocketIcon size={18} />
          </button>

          {groupDoc?.state === "paused" ? (
            <button
              onClick={() => void onSetGroupState("active")}
              disabled={!selectedGroupId || busy === "group-state"}
              className={classNames(
                "p-2 rounded-xl transition-all glass-btn",
                isDark ? "text-amber-400" : "text-amber-600"
              )}
              title="Resume Delivery"
            >
              <span className="sr-only">Resume</span>
              <PlayIcon size={18} />
            </button>
          ) : (
            <button
              onClick={() => void onSetGroupState("paused")}
              disabled={!selectedGroupId || busy === "group-state"}
              className={classNames(
                "p-2 rounded-xl transition-all glass-btn",
                isDark ? "text-slate-400 hover:text-amber-300" : "text-gray-400 hover:text-amber-600"
              )}
              title="Pause Delivery"
            >
              <span className="sr-only">Pause</span>
              <PauseIcon size={18} />
            </button>
          )}

          <button
            onClick={onStopGroup}
            disabled={!selectedGroupId || busy === "group-stop"}
            className={classNames(
              "p-2 rounded-xl transition-all glass-btn",
              isDark ? "text-slate-400 hover:text-rose-400" : "text-gray-400 hover:text-rose-600"
            )}
            title="Stop All Agents"
          >
            <span className="sr-only">Stop</span>
            <StopIcon size={18} />
          </button>
          <div className={`w-px h-4 mx-1 ${isDark ? "bg-white/10" : "bg-black/10"}`} />
        </div>

        <div className="hidden sm:block">
          <ThemeToggleCompact theme={theme} onThemeChange={onThemeChange} isDark={isDark} />
        </div>

        <button
          onClick={onOpenSettings}
          disabled={!selectedGroupId}
          className={classNames(
            "hidden sm:flex p-2 rounded-xl transition-all glass-btn",
            isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-400 hover:text-gray-600"
          )}
          title="Settings"
        >
          <SettingsIcon size={18} />
        </button>

        <button
          className={classNames(
            "sm:hidden flex items-center justify-center w-8 h-8 rounded-xl transition-all glass-btn",
            isDark ? "text-slate-400" : "text-gray-400"
          )}
          onClick={onOpenMobileMenu}
          title="Menu"
        >
          <MoreIcon size={18} />
        </button>
      </div>

      {/* Error Toast - Floating below header now */}
      {errorMsg && (
        <div className="absolute top-16 left-1/2 -translate-x-1/2 z-50 animate-slide-up">
          <div
            className={classNames(
              "rounded-2xl px-4 py-2.5 text-sm flex items-center gap-3 glass-modal",
              isDark
                ? "border-rose-500/20 text-rose-300"
                : "border-rose-200/50 text-rose-700"
            )}
            role="alert"
          >
            <span>{errorMsg}</span>
            <button 
              className={classNames(
                "p-1 rounded-lg transition-all glass-btn",
                isDark ? "text-rose-400" : "text-rose-600"
              )} 
              onClick={onDismissError}
            >
              ×
            </button>
          </div>
        </div>
      )}
    </header>
  );
}

import { GroupMeta } from "../../types";
import { getGroupStatus, getGroupStatusLight } from "../../utils/groupStatus";
import { classNames } from "../../utils/classNames";
import { CloseIcon, FolderIcon } from "../Icons";

export interface GroupSidebarProps {
  groups: GroupMeta[];
  selectedGroupId: string;
  isOpen: boolean;
  isDark: boolean;
  onSelectGroup: (groupId: string) => void;
  onCreateGroup: () => void;
  onClose: () => void;
}

export function GroupSidebar({
  groups,
  selectedGroupId,
  isOpen,
  isDark,
  onSelectGroup,
  onCreateGroup,
  onClose,
}: GroupSidebarProps) {
  return (
    <>
      <aside
        className={classNames(
          "h-full flex flex-col glass-sidebar",
          "fixed md:relative z-40 w-[280px] transition-all duration-300 ease-out",
          isOpen ? "translate-x-0" : "-translate-x-full",
          "md:translate-x-0"
        )}
      >
        <div className={`p-4 border-b ${isDark ? "border-white/5" : "border-black/5"}`}>
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className={classNames(
                "w-9 h-9 rounded-xl flex items-center justify-center glass-btn",
                isDark ? "text-cyan-400" : "text-cyan-600"
              )}>
                <img src="/ui/logo.svg" alt="CCCC Logo" className="w-6 h-6 object-contain" />
              </div>
              <span className={`text-lg font-bold tracking-tight ${isDark ? "text-white" : "text-gray-900"}`}>CCCC</span>
            </div>
            <div className="flex items-center gap-2">
              <button
                className={classNames(
                  "text-xs px-4 py-2 rounded-xl font-medium transition-all min-h-[36px] glass-btn-accent",
                  isDark ? "text-cyan-300" : "text-cyan-700"
                )}
                onClick={onCreateGroup}
                title="Create new working group"
                aria-label="Create new working group"
              >
                + New
              </button>
              <button
                className={classNames(
                  "md:hidden p-2 min-w-[44px] min-h-[44px] flex items-center justify-center rounded-xl transition-all glass-btn",
                  isDark ? "text-slate-400 hover:text-white" : "text-gray-500 hover:text-gray-900"
                )}
                onClick={onClose}
                aria-label="Close sidebar"
              >
                <CloseIcon size={18} />
              </button>
            </div>
          </div>
        </div>

        <div className="flex-1 overflow-auto p-3">
          <div className={`text-[10px] font-medium uppercase tracking-wider mb-3 px-2 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
            Working Groups
          </div>
          <div className="space-y-1.5">
            {groups.map((g) => {
              const gid = String(g.group_id || "");
              const active = gid === selectedGroupId;
              return (
                <button
                  key={gid}
                  className={classNames(
                    "w-full text-left px-3 py-2.5 rounded-xl transition-all min-h-[44px] group",
                    active
                      ? "glass-btn-accent glow-pulse"
                      : "glass-btn hover:translate-x-1"
                  )}
                  onClick={() => {
                    onSelectGroup(gid);
                    if (window.matchMedia("(max-width: 767px)").matches) onClose();
                  }}
                >
                  <div className="flex items-center justify-between">
                    <div
                      className={classNames(
                        "text-sm font-medium truncate",
                        active 
                          ? (isDark ? "text-cyan-300" : "text-cyan-700") 
                          : isDark ? "text-slate-300 group-hover:text-white" : "text-gray-700 group-hover:text-gray-900"
                      )}
                    >
                      {g.title || gid}
                    </div>
                    {(() => {
                      const status = isDark ? getGroupStatus(g.running ?? false, g.state) : getGroupStatusLight(g.running ?? false, g.state);
                      return (
                        <div className={classNames(
                          "text-[9px] px-2 py-0.5 rounded-full font-medium backdrop-blur-sm",
                          status.colorClass
                        )}>
                          {status.label}
                        </div>
                      );
                    })()}
                  </div>
                </button>
              );
            })}
          </div>
          {!groups.length && (
            <div className="p-6 text-center">
              <div className={classNames(
                "w-16 h-16 mx-auto mb-4 rounded-2xl flex items-center justify-center glass-card",
                isDark ? "text-slate-400" : "text-gray-400"
              )}>
                <FolderIcon size={32} />
              </div>
              <div className={`text-sm mb-2 font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>No working groups yet</div>
              <div className={`text-xs mb-5 max-w-[200px] mx-auto leading-relaxed ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                A working group is a collaboration space where multiple AI agents work together on a project.
              </div>
              <button
                className={classNames(
                  "text-sm px-5 py-2.5 rounded-xl font-medium min-h-[44px] transition-all glass-btn-accent",
                  isDark ? "text-cyan-300" : "text-cyan-700"
                )}
                onClick={onCreateGroup}
              >
                Create Your First Group
              </button>
            </div>
          )}
        </div>
      </aside>

      {/* Sidebar overlay for mobile */}
      {isOpen && (
        <div
          className="fixed inset-0 z-30 md:hidden glass-overlay animate-fade-in"
          onClick={onClose}
          aria-hidden="true"
        />
      )}
    </>
  );
}

// TimingTab - 时间设置
import { inputClass, labelClass, primaryButtonClass } from "./types";

interface TimingTabProps {
  isDark: boolean;
  busy: boolean;
  nudgeSeconds: number;
  setNudgeSeconds: (v: number) => void;
  idleSeconds: number;
  setIdleSeconds: (v: number) => void;
  keepaliveSeconds: number;
  setKeepaliveSeconds: (v: number) => void;
  silenceSeconds: number;
  setSilenceSeconds: (v: number) => void;
  deliveryInterval: number;
  setDeliveryInterval: (v: number) => void;
  standupInterval: number;
  setStandupInterval: (v: number) => void;
  onSave: () => void;
}

export function TimingTab({
  isDark,
  busy,
  nudgeSeconds,
  setNudgeSeconds,
  idleSeconds,
  setIdleSeconds,
  keepaliveSeconds,
  setKeepaliveSeconds,
  silenceSeconds,
  setSilenceSeconds,
  deliveryInterval,
  setDeliveryInterval,
  standupInterval,
  setStandupInterval,
  onSave,
}: TimingTabProps) {
  return (
    <div className="space-y-4">
      <div>
        <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>Group Timing</h3>
        <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
          These settings apply to the current working group.
        </p>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className={labelClass(isDark)}>Nudge After (sec)</label>
          <input
            type="number"
            value={nudgeSeconds}
            onChange={(e) => setNudgeSeconds(Number(e.target.value))}
            className={inputClass(isDark)}
          />
        </div>
        <div>
          <label className={labelClass(isDark)}>Actor Idle (sec)</label>
          <input
            type="number"
            value={idleSeconds}
            onChange={(e) => setIdleSeconds(Number(e.target.value))}
            className={inputClass(isDark)}
          />
        </div>
        <div>
          <label className={labelClass(isDark)}>Keepalive (sec)</label>
          <input
            type="number"
            value={keepaliveSeconds}
            onChange={(e) => setKeepaliveSeconds(Number(e.target.value))}
            className={inputClass(isDark)}
          />
        </div>
        <div>
          <label className={labelClass(isDark)}>Silence (sec)</label>
          <input
            type="number"
            value={silenceSeconds}
            onChange={(e) => setSilenceSeconds(Number(e.target.value))}
            className={inputClass(isDark)}
          />
        </div>
        <div>
          <label className={labelClass(isDark)}>Delivery Interval (sec)</label>
          <input
            type="number"
            value={deliveryInterval}
            onChange={(e) => setDeliveryInterval(Number(e.target.value))}
            className={inputClass(isDark)}
          />
        </div>
        <div>
          <label className={labelClass(isDark)}>Standup Interval (sec)</label>
          <input
            type="number"
            value={standupInterval}
            onChange={(e) => setStandupInterval(Number(e.target.value))}
            className={inputClass(isDark)}
          />
          <p className={`text-xs mt-1 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
            Periodic review reminder (default 900 = 15 min)
          </p>
        </div>
      </div>

      <button onClick={onSave} disabled={busy} className={primaryButtonClass(busy)}>
        {busy ? "Saving..." : "Save Timing Settings"}
      </button>
    </div>
  );
}

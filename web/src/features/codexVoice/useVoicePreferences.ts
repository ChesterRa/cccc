import { useEffect, useRef, useState } from "react";
import {
  fetchVoicePreferences,
  saveVoicePreferences,
  type VoicePreferences,
} from "../../services/api/codexVoice";

export function useVoicePreferences(active: boolean) {
  const [value, setValue] = useState<VoicePreferences | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const lock = useRef(false);
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    void fetchVoicePreferences()
      .then((response) => {
        if (cancelled) return;
        if (response.ok) {
          setValue((current) =>
            current && current.revision > response.result.preferences.revision
              ? current
              : response.result.preferences,
          );
          setError("");
        } else setError(response.error.message);
      })
      .catch((error: unknown) => {
        if (!cancelled) setError(String(error));
      });
    return () => {
      cancelled = true;
    };
  }, [active]);
  const change = async (patch: Partial<VoicePreferences>) => {
    if (!value || lock.current) return;
    lock.current = true;
    setSaving(true);
    setSaved(false);
    setError("");
    try {
      const response = await saveVoicePreferences({ ...value, ...patch });
      if (response.ok) {
        setValue(response.result.preferences);
        setSaved(true);
      } else {
        setError(response.error.message);
        if (response.error.code === "voice_preferences_conflict") {
          const fresh = await fetchVoicePreferences();
          if (fresh.ok) setValue(fresh.result.preferences);
        }
      }
    } catch (error) {
      setError(String(error));
    } finally {
      lock.current = false;
      setSaving(false);
    }
  };
  return { value, saving, saved, error, change };
}

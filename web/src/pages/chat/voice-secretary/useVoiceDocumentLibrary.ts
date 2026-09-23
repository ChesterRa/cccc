import { useCallback, useEffect, useRef, useState } from "react";
import {
  voiceDocumentLibrary,
  type LibraryAction,
  type VoiceLibrary,
} from "../../../services/api/voiceDocumentLibrary";

const empty: VoiceLibrary = { documents: [], folders: [] };
export function useVoiceDocumentLibrary(groupId: string, documents: unknown, actionBusy: string) {
  const [data, setData] = useState<VoiceLibrary>(empty);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const requests = useRef({ generation: 0, mutating: false }).current;
  useEffect(() => {
    requests.generation++;
    setData(empty);
    setError("");
    setBusy(false);
    requests.mutating = false;
    return () => {
      // Retire reads and mutations owned by this mounted group, including callbacks.
      requests.generation++;
    };
  }, [groupId, requests]);
  useEffect(() => {
    if (!groupId || actionBusy || requests.mutating) return;
    const ticket = ++requests.generation;
    void voiceDocumentLibrary(groupId)
      .then((response) => {
        if (requests.generation !== ticket) return;
        if (response.ok) {
          setData(response.result);
          setError("");
        } else setError(response.error.message);
      })
      .catch((error) => {
        if (requests.generation === ticket) setError(String(error));
      });
    return () => {
      if (requests.generation === ticket) requests.generation++;
    };
  }, [groupId, documents, actionBusy, requests]);
  const mutate = useCallback(
    async (action: LibraryAction) => {
      if (requests.mutating) return false;
      requests.mutating = true;
      const ticket = ++requests.generation;
      setBusy(true);
      setError("");
      try {
        const response = await voiceDocumentLibrary(groupId, action);
        if (requests.generation !== ticket) return false;
        if (!response.ok) {
          setError(response.error.message);
          return false;
        }
        setData(response.result);
        return true;
      } catch (error) {
        if (requests.generation === ticket) setError(String(error));
        return false;
      } finally {
        if (requests.generation === ticket) {
          requests.mutating = false;
          setBusy(false);
        }
      }
    },
    [groupId, requests],
  );
  return { data, busy, error, mutate };
}

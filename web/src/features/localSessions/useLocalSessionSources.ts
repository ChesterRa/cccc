import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import type { LocalSessionSource } from "../../types";
import { fetchLocalSessions } from "../../services/api/localSessions";
import type {
  LocalSessionDirectory,
  SourceControlResult,
  SourceDiagnostic,
} from "../../services/api/localSessionControls";

export function useLocalSessionSources(
  setSelected: Dispatch<SetStateAction<LocalSessionSource[]>>,
) {
  const [directory, setDirectory] = useState<LocalSessionDirectory>({ sources: [] });
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const alive = useRef(false);
  const sequence = useRef(0);
  const reload = useCallback(async () => {
    const request = ++sequence.current;
    try {
      const response = await fetchLocalSessions();
      if (!alive.current || request !== sequence.current) return;
      if (response.ok) {
        setError("");
        setDirectory(response.result);
        setSelected((previous) =>
          previous
            .filter(
              (source) =>
                !response.result.source_states?.[source.source_id]?.control.access_blocked,
            )
            .map(
              (source) =>
                response.result.sources.find((latest) => latest.source_id === source.source_id) || {
                  ...source,
                  connected: false,
                },
            ),
        );
      } else setError(response.error.message);
    } catch (error) {
      if (alive.current && request === sequence.current) setError(String(error));
    } finally {
      if (alive.current && request === sequence.current) setLoading(false);
    }
  }, [setSelected]);
  useEffect(() => {
    alive.current = true;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const load = async () => {
      await reload();
      if (!cancelled) timer = setTimeout(() => void load(), 3000);
    };
    void load();
    return () => {
      cancelled = true;
      alive.current = false;
      sequence.current += 1;
      clearTimeout(timer);
    };
  }, [reload]);
  const apply = (source: LocalSessionSource, result: SourceControlResult) => {
    sequence.current += 1;
    if (result.access_blocked)
      setSelected((previous) => previous.filter((item) => item.source_id !== source.source_id));
    setDirectory((previous) => {
      const state = previous.source_states?.[source.source_id];
      return {
        ...previous,
        sources: result.access_blocked
          ? previous.sources.filter((item) => item.source_id !== source.source_id)
          : [
              ...previous.sources.filter((item) => item.source_id !== source.source_id),
              { ...source, connected: false },
            ],
        removed_sources: result.access_blocked
          ? [
              ...(previous.removed_sources || []).filter(
                (item) => item.source_id !== source.source_id,
              ),
              source,
            ]
          : (previous.removed_sources || []).filter((item) => item.source_id !== source.source_id),
        source_states: {
          ...previous.source_states,
          [source.source_id]: {
            diagnostic: state?.diagnostic || { state: "unknown", checked_at: null },
            control: {
              ...state?.control,
              supported: state?.control.supported ?? true,
              management_revision: result.management_revision,
              access_blocked: result.access_blocked,
              pause_complete: result.pause_complete,
            },
          },
        },
      };
    });
  };
  const diagnose = (sourceId: string, diagnostic: SourceDiagnostic) => {
    sequence.current += 1;
    setDirectory((previous) => {
      const state = previous.source_states?.[sourceId];
      return state
        ? {
            ...previous,
            source_states: { ...previous.source_states, [sourceId]: { ...state, diagnostic } },
          }
        : previous;
    });
  };
  return {
    ...directory,
    loading,
    error,
    reload,
    apply,
    diagnose,
    invalidate: () => {
      sequence.current += 1;
    },
  };
}

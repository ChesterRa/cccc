import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import type { DeviceSessionSource, LocalSessionSource } from "../../types";
import { fetchLocalSessions } from "../../services/api/localSessions";
import { fetchDeviceSessionSources } from "../../services/api/sessionNodes";
import type {
  LocalSessionDirectory,
  SourceControlResult,
  SourceDiagnostic,
} from "../../services/api/localSessionControls";
import { isDeviceSessionSource } from "./remoteSessionPolicy";

export function useLocalSessionSources({
  active,
  includeDeviceSources,
  setSelected,
}: {
  active: boolean;
  includeDeviceSources: boolean;
  setSelected?: Dispatch<SetStateAction<LocalSessionSource[]>>;
}) {
  const [localDirectory, setLocalDirectory] = useState<LocalSessionDirectory>({ sources: [] });
  const [deviceSources, setDeviceSources] = useState<DeviceSessionSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [deviceLoading, setDeviceLoading] = useState(true);
  const [error, setError] = useState("");
  const [deviceError, setDeviceError] = useState("");
  const alive = useRef(false);
  const localSequence = useRef(0);
  const deviceSequence = useRef(0);

  const reloadLocal = useCallback(async () => {
    if (!alive.current) return;
    const request = ++localSequence.current;
    try {
      const response = await fetchLocalSessions();
      if (!alive.current || request !== localSequence.current) return;
      if (response.ok) {
        setError("");
        setLocalDirectory(response.result);
        const nextLocalSources = response.result.sources;
        const nextLocalStates = response.result.source_states;
        setSelected?.((previous) =>
          previous
            .filter((source) =>
              isDeviceSessionSource(source)
                ? true
                : !nextLocalStates?.[source.source_id]?.control.access_blocked,
            )
            .map((source) => {
              if (isDeviceSessionSource(source)) return source;
              return (
                nextLocalSources.find((latest) => latest.source_id === source.source_id) || {
                  ...source,
                  connected: false,
                }
              );
            }),
        );
      } else {
        setError(response.error.message);
      }
    } catch (reason) {
      if (alive.current && request === localSequence.current) setError(String(reason));
    } finally {
      if (alive.current && request === localSequence.current) setLoading(false);
    }
  }, [setSelected]);

  const reloadDevice = useCallback(async () => {
    const request = ++deviceSequence.current;
    try {
      const response = await fetchDeviceSessionSources();
      if (!alive.current || request !== deviceSequence.current) return;
      let nextDeviceSources: DeviceSessionSource[] | null = null;
      if (response.ok) {
        setDeviceError("");
        nextDeviceSources = Array.isArray(response.result?.sources) ? response.result.sources : [];
        setDeviceSources(nextDeviceSources);
      } else if (response.error.httpStatus === 404) {
        setDeviceError("");
        nextDeviceSources = [];
        setDeviceSources([]);
      } else {
        setDeviceError(response.error.message);
      }
      if (nextDeviceSources !== null) {
        const latestSources = nextDeviceSources;
        setSelected?.((previous) =>
          previous.map((source) => {
            if (!isDeviceSessionSource(source)) return source;
            return (
              latestSources.find((latest) => latest.source_id === source.source_id) || {
                ...source,
                connected: false,
              }
            );
          }),
        );
      }
    } catch (reason) {
      if (alive.current && request === deviceSequence.current) setDeviceError(String(reason));
    } finally {
      if (alive.current && request === deviceSequence.current) setDeviceLoading(false);
    }
  }, [setSelected]);

  useEffect(() => {
    if (!active) return;
    alive.current = true;
    setLoading(true);
    if (includeDeviceSources) setDeviceLoading(true);
    let cancelled = false;
    let localTimer: ReturnType<typeof setTimeout> | undefined;
    let deviceTimer: ReturnType<typeof setTimeout> | undefined;
    const loadLocal = async () => {
      await reloadLocal();
      if (!cancelled) localTimer = setTimeout(() => void loadLocal(), 3000);
    };
    const loadDevice = async () => {
      await reloadDevice();
      if (!cancelled) deviceTimer = setTimeout(() => void loadDevice(), 3000);
    };
    void loadLocal();
    if (includeDeviceSources) void loadDevice();
    return () => {
      cancelled = true;
      alive.current = false;
      localSequence.current += 1;
      deviceSequence.current += 1;
      clearTimeout(localTimer);
      clearTimeout(deviceTimer);
    };
  }, [active, includeDeviceSources, reloadLocal, reloadDevice]);

  const apply = (source: LocalSessionSource, result: SourceControlResult) => {
    localSequence.current += 1;
    if (result.access_blocked)
      setSelected?.((previous) => previous.filter((item) => item.source_id !== source.source_id));
    setLocalDirectory((previous) => {
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
    localSequence.current += 1;
    setLocalDirectory((previous) => {
      const state = previous.source_states?.[sourceId];
      return state
        ? {
            ...previous,
            source_states: { ...previous.source_states, [sourceId]: { ...state, diagnostic } },
          }
        : previous;
    });
  };

  const sources = useMemo(
    () =>
      includeDeviceSources ? [...localDirectory.sources, ...deviceSources] : localDirectory.sources,
    [localDirectory.sources, deviceSources, includeDeviceSources],
  );

  return {
    ...localDirectory,
    sources,
    loading,
    deviceLoading: includeDeviceSources && deviceLoading,
    error,
    deviceError,
    reload: reloadLocal,
    apply,
    diagnose,
    invalidate: () => {
      localSequence.current += 1;
      deviceSequence.current += 1;
    },
  };
}

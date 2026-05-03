export function voiceTranscriptSourceLabel(source: string): string {
  const value = String(source || "").trim();
  if (value === "assistant_service_local_asr_final") return "Final SenseVoice";
  if (value === "assistant_service_local_asr_streaming") return "Live Paraformer";
  if (value === "browser_asr") return "Browser ASR";
  return value ? "ASR" : "";
}

export function voiceTranscriptSourceDetail(params: {
  source?: string;
  modelId?: string;
  engine?: string;
  language?: string;
  chunks?: number;
  fallbackReason?: string;
}): string {
  const parts = [
    params.modelId,
    params.engine,
    params.language ? `lang=${params.language}` : "",
    params.chunks ? `${params.chunks} chunks` : "",
    params.fallbackReason ? `fallback=${params.fallbackReason}` : "",
  ].filter((part) => String(part || "").trim());
  return parts.join(" · ");
}

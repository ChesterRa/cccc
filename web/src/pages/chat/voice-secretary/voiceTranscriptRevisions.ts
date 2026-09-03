type TranscriptRecord = Record<string, unknown>;

export function projectVoiceTranscriptRevisions(segments: TranscriptRecord[]): TranscriptRecord[] {
  const superseded = new Set(
    segments.flatMap((segment) => {
      const sessionId = String(segment.session_id || "").trim();
      return Array.isArray(segment.supersedes_segment_ids)
        ? segment.supersedes_segment_ids
            .map((value) => String(value || "").trim())
            .filter(Boolean)
            .map((segmentId) => revisionKey(sessionId, segmentId))
        : [];
    }),
  );
  const supersedesLiveSessions = new Set(
    segments
      .filter(
        (segment) => segment.transcript_stage === "final" && segment.supersede_stage === "live",
      )
      .map((segment) => String(segment.session_id || "").trim()),
  );
  return segments.filter((segment) => {
    const segmentId = String(segment.segment_id || "").trim();
    const sessionId = String(segment.session_id || "").trim();
    return (
      (!segmentId || !superseded.has(revisionKey(sessionId, segmentId))) &&
      (!supersedesLiveSessions.has(sessionId) || transcriptStage(segment) !== "live")
    );
  });
}

function revisionKey(sessionId: string, segmentId: string): string {
  return `${sessionId}\u0000${segmentId}`;
}

function transcriptStage(segment: TranscriptRecord): "live" | "final" {
  if (segment.transcript_stage === "final") return "final";
  const trigger =
    segment.trigger && typeof segment.trigger === "object"
      ? (segment.trigger as TranscriptRecord)
      : {};
  return trigger.recognition_backend === "assistant_service_local_asr_final" ? "final" : "live";
}

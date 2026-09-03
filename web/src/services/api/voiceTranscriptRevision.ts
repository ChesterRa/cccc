export type VoiceTranscriptRevisionPayload = {
  stage: "live" | "final";
  revisionOnly?: boolean;
  supersedeStage?: "live";
  supersedesSegmentIds?: string[];
  sourceModelId?: string;
};

export function voiceTranscriptRevisionBody(
  revision?: VoiceTranscriptRevisionPayload,
): Record<string, unknown> {
  if (!revision) return {};
  return {
    transcript_stage: revision.stage,
    revision_only: Boolean(revision.revisionOnly),
    supersede_stage: String(revision.supersedeStage || "").trim(),
    supersedes_segment_ids: (revision.supersedesSegmentIds || [])
      .map((value) => String(value || "").trim())
      .filter(Boolean),
    source_model_id: String(revision.sourceModelId || "").trim(),
  };
}

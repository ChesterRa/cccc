use cccc_core::HomeLayout;

use super::{voice_asr, voice_pcm_recording::PcmRecording};

const PCM16_BYTES_PER_SECOND: usize = 16_000 * 2;
pub(super) const DEFAULT_SEGMENT_DURATION_MS: u64 = 25 * 60 * 1_000;
const DEFAULT_SEGMENT_BYTES: usize = 25 * 60 * PCM16_BYTES_PER_SECOND;
const MAX_STREAMING_SESSION_BYTES: usize = voice_asr::MAX_AUDIO_BYTES * 8;

pub(super) struct RecordingSegment {
    pub(super) file: tempfile::NamedTempFile,
    pub(super) index: usize,
    pub(super) start_ms: u64,
    pub(super) end_ms: u64,
    pub(super) bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SegmentBoundary {
    pub(super) index: usize,
    pub(super) start_ms: u64,
    pub(super) end_ms: u64,
    pub(super) bytes: usize,
}

pub(super) struct SegmentedPcmRecording {
    home: HomeLayout,
    active: Option<PcmRecording>,
    completed: Vec<RecordingSegment>,
    total_bytes: usize,
    segment_bytes: usize,
    session_limit: usize,
}

impl SegmentedPcmRecording {
    pub(super) fn create(home: &HomeLayout) -> Result<Self, voice_asr::VoiceError> {
        Self::create_with_limits(home, DEFAULT_SEGMENT_BYTES, MAX_STREAMING_SESSION_BYTES)
    }

    fn create_with_limits(
        home: &HomeLayout,
        segment_bytes: usize,
        session_limit: usize,
    ) -> Result<Self, voice_asr::VoiceError> {
        if segment_bytes == 0 || segment_bytes % 2 != 0 || session_limit < segment_bytes {
            return Err(voice_asr::VoiceError::new(
                "invalid_audio_limit",
                "PCM16 recording limits must be positive, aligned, and ordered",
            ));
        }
        Ok(Self {
            home: home.clone(),
            active: Some(PcmRecording::create_with_limit(home, segment_bytes)?),
            completed: Vec::new(),
            total_bytes: 0,
            segment_bytes,
            session_limit,
        })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.total_bytes == 0
    }

    pub(super) async fn append(
        &mut self,
        pcm: &[u8],
    ) -> Result<Vec<SegmentBoundary>, voice_asr::VoiceError> {
        if pcm.len() % 2 != 0 {
            return Err(voice_asr::VoiceError::new(
                "invalid_audio",
                "PCM16 byte length must be even",
            ));
        }
        if self.total_bytes.saturating_add(pcm.len()) > self.session_limit {
            return Err(voice_asr::VoiceError::new(
                "audio_too_large",
                "streaming session exceeds the segmented recording limit",
            ));
        }

        let mut remaining = pcm;
        let mut boundaries = Vec::new();
        while !remaining.is_empty() {
            self.ensure_active()?;
            let active_bytes = self.active.as_ref().map_or(0, PcmRecording::bytes);
            let writable = (self.segment_bytes - active_bytes).min(remaining.len());
            if let Some(active) = self.active.as_mut() {
                active.append(&remaining[..writable]).await?;
            }
            self.total_bytes += writable;
            remaining = &remaining[writable..];
            if self
                .active
                .as_ref()
                .is_some_and(|active| active.bytes() == self.segment_bytes)
            {
                boundaries.push(self.finish_active().await?);
            }
        }
        Ok(boundaries)
    }

    pub(super) async fn finish(mut self) -> Result<Vec<RecordingSegment>, voice_asr::VoiceError> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| !active.is_empty())
        {
            self.finish_active().await?;
        }
        Ok(self.completed)
    }

    fn ensure_active(&mut self) -> Result<(), voice_asr::VoiceError> {
        if self.active.is_none() {
            self.active = Some(PcmRecording::create_with_limit(
                &self.home,
                self.segment_bytes,
            )?);
        }
        Ok(())
    }

    async fn finish_active(&mut self) -> Result<SegmentBoundary, voice_asr::VoiceError> {
        let active = self.active.take().ok_or_else(|| {
            voice_asr::VoiceError::new("recording_not_started", "recording segment is missing")
        })?;
        let bytes = active.bytes();
        let end_ms = duration_ms(self.total_bytes);
        let start_ms = duration_ms(self.total_bytes.saturating_sub(bytes));
        let index = self.completed.len() + 1;
        let file = active.finish().await?;
        self.completed.push(RecordingSegment {
            file,
            index,
            start_ms,
            end_ms,
            bytes,
        });
        Ok(SegmentBoundary {
            index,
            start_ms,
            end_ms,
            bytes,
        })
    }
}

fn duration_ms(bytes: usize) -> u64 {
    u64::try_from(bytes)
        .unwrap_or(u64::MAX)
        .saturating_mul(1_000)
        / PCM16_BYTES_PER_SECOND as u64
}

#[cfg(test)]
mod tests;

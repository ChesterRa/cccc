use cccc_core::HomeLayout;
use tokio::io::AsyncWriteExt;

use super::voice_asr;

pub(super) struct PcmRecording {
    file: tempfile::NamedTempFile,
    output: tokio::io::BufWriter<tokio::fs::File>,
    bytes: usize,
    limit: usize,
}

impl PcmRecording {
    #[cfg(test)]
    pub(super) fn create(home: &HomeLayout) -> Result<Self, voice_asr::VoiceError> {
        Self::create_with_limit(home, voice_asr::MAX_AUDIO_BYTES)
    }

    pub(super) fn create_with_limit(
        home: &HomeLayout,
        limit: usize,
    ) -> Result<Self, voice_asr::VoiceError> {
        let temp_dir = home.root().join("cache/voice-ws-recordings");
        std::fs::create_dir_all(&temp_dir).map_err(write_error)?;
        let file = tempfile::NamedTempFile::new_in(temp_dir).map_err(write_error)?;
        let output = tokio::io::BufWriter::new(tokio::fs::File::from_std(
            file.reopen().map_err(write_error)?,
        ));
        Ok(Self {
            file,
            output,
            bytes: 0,
            limit,
        })
    }

    pub(super) async fn append(&mut self, pcm: &[u8]) -> Result<(), voice_asr::VoiceError> {
        if pcm.len() % 2 != 0 {
            return Err(voice_asr::VoiceError::new(
                "invalid_audio",
                "PCM16 byte length must be even",
            ));
        }
        let next = self.bytes.saturating_add(pcm.len());
        if next > self.limit {
            return Err(voice_asr::VoiceError::new(
                "audio_too_large",
                "streaming audio exceeds the recording file limit",
            ));
        }
        self.output.write_all(pcm).await.map_err(write_error)?;
        self.bytes = next;
        Ok(())
    }

    pub(super) fn is_empty(&self) -> bool {
        self.bytes == 0
    }

    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(super) async fn finish(mut self) -> Result<tempfile::NamedTempFile, voice_asr::VoiceError> {
        self.output.flush().await.map_err(write_error)?;
        self.output
            .get_ref()
            .sync_data()
            .await
            .map_err(write_error)?;
        drop(self.output);
        Ok(self.file)
    }
}

fn write_error(error: std::io::Error) -> voice_asr::VoiceError {
    voice_asr::VoiceError::new("audio_write_failed", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recording_streams_to_an_auto_deleted_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let mut recording = PcmRecording::create(&home).expect("recording");
        recording.append(&[1, 2]).await.expect("first chunk");
        recording.append(&[3, 4]).await.expect("second chunk");
        let file = recording.finish().await.expect("finish");
        assert_eq!(std::fs::read(file.path()).expect("read"), [1, 2, 3, 4]);
        let path = file.path().to_owned();
        drop(file);
        assert!(!path.exists());

        let mut limited = PcmRecording::create_with_limit(&home, 3).expect("limited recording");
        limited.append(&[1, 2]).await.expect("within limit");
        let error = limited.append(&[3, 4]).await.expect_err("over limit");
        assert_eq!(error.code, "audio_too_large");

        let mut aligned = PcmRecording::create_with_limit(&home, 4).expect("aligned recording");
        let error = aligned.append(&[1]).await.expect_err("odd PCM16 chunk");
        assert_eq!(error.code, "invalid_audio");
        aligned.append(&[2, 3]).await.expect("valid PCM16 chunk");
        let file = aligned.finish().await.expect("finish aligned recording");
        assert_eq!(std::fs::read(file.path()).expect("read"), [2, 3]);
    }
}

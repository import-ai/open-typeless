use std::{io::Cursor, path::Path};

pub const MAX_AUDIO_BYTES: usize = 12 << 20;
pub const MAX_RECORDING_SECONDS: u64 = 240;

// One bounded source for live PCM, the fallback WAV, and archived audio.
pub struct Audio {
    samples: Vec<i16>,
    channels: usize,
    sample_rate: u32,
    max_samples: usize,
    next_window: usize,
    speech_start: Option<usize>,
    sent: usize,
}

impl Audio {
    pub fn new(sample_rate: u32, channels: u16, max_bytes: usize) -> Result<Self, String> {
        if sample_rate == 0 || channels == 0 || max_bytes < 44 + channels as usize * 2 {
            return Err("后端录音大小限制或麦克风格式无效".into());
        }
        let channels = channels as usize;
        let max_samples = ((max_bytes.min(MAX_AUDIO_BYTES) - 44) / (2 * channels)) * channels;
        Ok(Self {
            samples: Vec::new(),
            channels,
            sample_rate,
            max_samples,
            next_window: 0,
            speech_start: None,
            sent: 0,
        })
    }

    pub fn push(&mut self, samples: impl Iterator<Item = i16>) {
        self.samples
            .extend(samples.take(self.max_samples - self.samples.len()));
    }

    pub fn full(&self) -> bool {
        self.samples.len() == self.max_samples
    }

    fn detect_speech(&mut self, finishing: bool) {
        let frames = self.samples.len() / self.channels;
        let window = (self.sample_rate / 100).max(1) as usize;
        let windows = if finishing {
            frames.div_ceil(window)
        } else {
            frames / window
        };
        let threshold = 32768.0_f64 * 10_f64.powf(-42.0 / 20.0);
        while self.speech_start.is_none() && self.next_window + 1 < windows {
            let start = self.next_window * window;
            if crate::rms(
                &self.samples,
                self.channels,
                start,
                (start + window).min(frames),
            ) > threshold
                && crate::rms(
                    &self.samples,
                    self.channels,
                    start + window,
                    (start + 2 * window).min(frames),
                ) > threshold
            {
                self.speech_start =
                    Some(start.saturating_sub(self.sample_rate as usize / 10) * self.channels);
            }
            self.next_window += 1;
        }
    }

    pub fn pending(&mut self, finishing: bool) -> Vec<i16> {
        self.detect_speech(finishing);
        let Some(start) = self.speech_start else {
            return Vec::new();
        };
        let end = self.samples.len() / self.channels * self.channels;
        let chunk = self.samples[self.sent.max(start)..end].to_vec();
        self.sent = end;
        chunk
    }

    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        self.detect_speech(true);
        let start = self.speech_start.ok_or("未检测到有效音频")?;
        let end = self.samples.len() / self.channels * self.channels;
        let mut data = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(
            &mut data,
            hound::WavSpec {
                channels: self.channels as u16,
                sample_rate: self.sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .map_err(|e| e.to_string())?;
        for sample in &self.samples[start..end] {
            writer.write_sample(*sample).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
        Ok(data.into_inner())
    }
}

pub fn save(path: &Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut staged = tempfile::NamedTempFile::new_in(path.parent().ok_or("录音路径无效")?)
        .map_err(|e| e.to_string())?;
    staged.write_all(data).map_err(|e| e.to_string())?;
    staged.as_file().sync_all().map_err(|e| e.to_string())?;
    staged.persist_noclobber(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_wav_and_archive_have_identical_samples_including_tail() {
        for (rate, channels) in [(16000, 1), (48000, 2)] {
            let mut audio = Audio::new(rate, channels, MAX_AUDIO_BYTES).unwrap();
            let mut source = vec![0; rate as usize * channels as usize];
            source.extend(vec![1000; rate as usize * channels as usize]);
            source.extend(vec![321; rate as usize * channels as usize / 5]);
            let mut live = Vec::new();
            // Deliberately cross sample-frame and speech-window boundaries.
            for chunk in source.chunks(137) {
                audio.push(chunk.iter().copied());
                live.extend(audio.pending(false));
            }
            live.extend(audio.pending(true));
            let wav = audio.finish().unwrap();
            let decoded = hound::WavReader::new(Cursor::new(&wav))
                .unwrap()
                .samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(decoded, live);
            assert_eq!(
                decoded,
                source[rate as usize * channels as usize * 9 / 10..]
            );
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("recording.wav");
            save(&path, &wav).unwrap();
            assert_eq!(std::fs::read(path).unwrap(), wav);
        }
    }

    #[test]
    fn bounds_memory_and_wav_size_and_rejects_silence() {
        let mut audio = Audio::new(48000, 2, 4000).unwrap();
        audio.push(std::iter::repeat(1000));
        assert!(audio.full());
        audio.push(std::iter::repeat(1000));
        assert!(audio.finish().unwrap().len() <= 4000);
        let mut silence = Audio::new(16000, 1, 4000).unwrap();
        silence.push(std::iter::repeat(0));
        assert!(silence.pending(true).is_empty());
        assert!(silence.finish().is_err());
        assert!(Audio::new(16000, 1, 44).is_err());
    }

    #[test]
    fn failed_publication_preserves_existing_file_and_removes_staging() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recording.wav");
        std::fs::write(&path, b"existing").unwrap();
        assert!(save(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"existing");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert!(save(&directory.path().join("missing/recording.wav"), b"audio").is_err());
    }
}

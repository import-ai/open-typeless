//! Xiaomi firmware 2671: ATVV v1.0 controls the complete recording lifecycle.
//! Protocol/ADPCM adapted from fanxeon/mi-ao (MIT); see THIRD_PARTY_NOTICES.md.
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};

use crate::{recording::Audio, TranscriptionRun};

pub const SHORTCUT: &str = "XiaomiRemote";

// IMA ADPCM uses the high nibble first on this remote.
#[derive(Default)]
struct Decoder {
    predictor: i32,
    index: usize,
}
impl Decoder {
    fn decode(&mut self, data: &[u8]) -> Vec<i16> {
        const INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];
        const STEP: [i32; 89] = [
            7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55,
            60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307,
            337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411,
            1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358,
            5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500,
            20350, 22385, 24623, 27086, 29794, 32767,
        ];
        let mut samples = Vec::with_capacity(data.len() * 2);
        for nibble in data.iter().flat_map(|byte| [byte >> 4, byte & 15]) {
            let step = STEP[self.index];
            let difference = (step >> 3)
                + if nibble & 4 != 0 { step } else { 0 }
                + if nibble & 2 != 0 { step >> 1 } else { 0 }
                + if nibble & 1 != 0 { step >> 2 } else { 0 };
            self.predictor = (self.predictor
                + if nibble & 8 != 0 {
                    -difference
                } else {
                    difference
                })
            .clamp(i16::MIN as i32, i16::MAX as i32);
            self.index = (self.index as i32 + INDEX[(nibble & 7) as usize]).clamp(0, 88) as usize;
            samples.push(self.predictor as i16);
        }
        samples
    }
}

fn capabilities(data: &[u8]) -> Result<(), String> {
    if data.len() < 7 || data[..3] != [0x0b, 1, 0] {
        return Err("仅支持 ATVV v1.0 小米遥控器".into());
    }
    let codec = if data[3] & 3 == 0 { data[4] } else { data[3] };
    if codec & 2 == 0 || u16::from_be_bytes([data[5], data[6]]) != 120 {
        return Err("遥控器未提供 ADPCM 16 kHz / 120 字节音频".into());
    }
    Ok(())
}

struct Sink {
    run: Arc<TranscriptionRun>,
    audio: Arc<Mutex<Audio>>,
    ended: mpsc::Sender<()>,
}

struct Session {
    id: u64,
    stream_id: u8,
    decoder: Decoder,
    pending: Vec<i16>,
    ended: bool,
    sink: Option<Sink>,
}
impl Session {
    fn feed(&mut self, data: &[u8]) -> Result<f64, String> {
        if self.ended
            || self
                .sink
                .as_ref()
                .is_some_and(|sink| sink.run.check().is_err())
        {
            return Ok(0.0);
        }
        if data.len() != 120 {
            return Err("遥控器音频帧长度无效".into());
        }
        let samples = self.decoder.decode(data);
        let level = samples
            .iter()
            .map(|sample| (*sample as f64).abs())
            .sum::<f64>()
            / (samples.len() as f64 * 32768.0);
        if let Some(sink) = self.sink.as_ref() {
            sink.audio
                .lock()
                .map_err(|e| e.to_string())?
                .push(samples.into_iter());
        } else if self.pending.len() + samples.len()
            <= 16000 * crate::recording::MAX_RECORDING_SECONDS as usize
        {
            self.pending.extend(samples);
        } else {
            return Err("遥控器音频缓冲已满，请松开语音键后重试".into());
        }
        Ok(level)
    }

    fn finish(&mut self) -> bool {
        if self.ended {
            return false;
        }
        self.ended = true;
        if let Some(sink) = self.sink.as_ref() {
            let _ = sink.ended.send(());
        }
        true
    }

    fn attach(
        &mut self,
        run: Arc<TranscriptionRun>,
        audio: Arc<Mutex<Audio>>,
    ) -> Result<mpsc::Receiver<()>, String> {
        run.check()?;
        if self.sink.is_some() {
            return Err("遥控器已有录音进行中".into());
        }
        audio
            .lock()
            .map_err(|e| e.to_string())?
            .push(std::mem::take(&mut self.pending).into_iter());
        let (ended, result) = mpsc::channel();
        if self.ended {
            let _ = ended.send(());
        }
        self.sink = Some(Sink { run, audio, ended });
        Ok(result)
    }
}

enum Start {
    New(u64),
    Duplicate,
    Ignored,
}
#[derive(Default)]
struct Sessions {
    next_id: u64,
    current: Option<Session>,
    ignoring: bool,
}
impl Sessions {
    fn start(&mut self, stream_id: u8, blocked: bool) -> Result<Start, String> {
        if self.ignoring {
            return Ok(Start::Ignored);
        }
        if let Some(session) = self.current.as_ref().filter(|session| !session.ended) {
            return if session.stream_id == stream_id {
                Ok(Start::Duplicate)
            } else {
                Err("遥控器在上次录音结束前切换了音频流".into())
            };
        }
        if blocked
            || self
                .current
                .as_ref()
                .is_some_and(|session| session.sink.is_some())
        {
            self.ignoring = true;
            return Ok(Start::Ignored);
        }
        self.next_id += 1;
        self.current = Some(Session {
            id: self.next_id,
            stream_id,
            decoder: Decoder::default(),
            pending: Vec::new(),
            ended: false,
            sink: None,
        });
        Ok(Start::New(self.next_id))
    }

    fn stop(&mut self) -> Option<u64> {
        if self.ignoring {
            self.ignoring = false;
            return None;
        }
        let session = self.current.as_mut()?;
        session.finish().then_some(session.id)
    }

    fn discard(&mut self, id: u64) -> bool {
        if self.current.as_ref().is_none_or(|session| session.id != id) {
            return false;
        }
        let session = self.current.take().unwrap();
        if !session.ended {
            self.ignoring = true;
        }
        true
    }
}

struct Remote {
    app: AppHandle,
    enabled: bool,
    ready: bool,
    status: String,
    sessions: Sessions,
}
static REMOTE: OnceLock<Mutex<Remote>> = OnceLock::new();

extern "C" {
    fn ot_remote_enable(callback: extern "C" fn(u32, *const u8, usize), enabled: bool);
    fn ot_remote_close();
    fn ot_remote_reset();
}

pub fn install(app: &AppHandle) {
    let _ = REMOTE.set(Mutex::new(Remote {
        app: app.clone(),
        enabled: false,
        ready: false,
        status: "小米遥控器未启用".into(),
        sessions: Sessions::default(),
    }));
}

pub fn configure(enabled: bool) -> Result<(), String> {
    let mut remote = REMOTE
        .get()
        .ok_or("遥控器监听未初始化")?
        .lock()
        .map_err(|e| e.to_string())?;
    if remote.enabled == enabled {
        return Ok(());
    }
    remote.enabled = enabled;
    remote.ready = false;
    if let Some(sink) = remote
        .sessions
        .current
        .as_ref()
        .and_then(|session| session.sink.as_ref())
    {
        sink.run.cancel();
    }
    remote.sessions.current = None;
    remote.sessions.ignoring = false;
    remote.status = if enabled {
        "正在连接小米遥控器…"
    } else {
        "小米遥控器未启用"
    }
    .into();
    unsafe {
        ot_remote_enable(receive, enabled);
    }
    Ok(())
}

pub fn status() -> String {
    REMOTE
        .get()
        .and_then(|remote| remote.lock().ok().map(|r| r.status.clone()))
        .unwrap_or_else(|| "遥控器监听未初始化".into())
}

fn failure(remote: &mut Remote, message: String) {
    remote.status = message.clone();
    remote.ready = false;
    remote.sessions.ignoring = false;
    if let Some(session) = remote.sessions.current.as_mut() {
        session.pending.clear();
        if let Some(sink) = session.sink.as_ref() {
            *sink.run.capture_error.lock().unwrap() = Some(message);
        }
        session.finish();
        let _ = remote
            .app
            .emit_to("main", "remote-stop-requested", session.id);
    }
    unsafe {
        ot_remote_reset();
    }
}

extern "C" fn receive(kind: u32, bytes: *const u8, length: usize) {
    // Native callback payloads remain valid only until this function returns.
    if bytes.is_null() || length == 0 || length > 4096 {
        return;
    }
    let data = unsafe { std::slice::from_raw_parts(bytes, length) };
    let Some(mut remote) = REMOTE.get().and_then(|remote| remote.lock().ok()) else {
        return;
    };
    if !remote.enabled {
        return;
    }
    let app = remote.app.clone();
    let old_status = remote.status.clone();
    match kind {
        1 => remote.status = String::from_utf8_lossy(data).into_owned(),
        3 => match data[0] {
            0x0b => match capabilities(data) {
                Ok(()) => {
                    remote.ready = true;
                    remote.status = "小米遥控器已就绪 · 按住语音键说话，松开后识别".into();
                }
                Err(error) => failure(&mut remote, error),
            },
            4 => {
                if data.len() < 4 || data[2] != 2 {
                    failure(&mut remote, "遥控器返回了不支持的录音格式".into());
                } else {
                    let state = app.state::<crate::AppState>();
                    let blocked = !remote.ready
                        || state
                            .shortcut_capturing
                            .try_lock()
                            .map(|v| *v)
                            .unwrap_or(true)
                        || state
                            .active
                            .try_lock()
                            .map(|run| run.is_some())
                            .unwrap_or(true);
                    match remote.sessions.start(data[3], blocked) {
                        Ok(Start::New(id)) => {
                            let _ = app.emit_to("main", "remote-start-requested", id);
                        }
                        Ok(Start::Duplicate) => (),
                        Ok(Start::Ignored) => unsafe {
                            ot_remote_close();
                        },
                        Err(error) => failure(&mut remote, error),
                    }
                }
            }
            0x0a if !remote.sessions.ignoring => {
                if data.len() != 7 || data[1] != 2 || data[6] > 88 {
                    failure(&mut remote, "遥控器音频同步数据无效".into());
                } else if let Some(session) = remote
                    .sessions
                    .current
                    .as_mut()
                    .filter(|session| !session.ended)
                {
                    session.decoder = Decoder {
                        predictor: i16::from_be_bytes([data[4], data[5]]) as i32,
                        index: data[6] as usize,
                    };
                }
            }
            0 => {
                if let Some(id) = remote.sessions.stop() {
                    let _ = app.emit_to("main", "remote-stop-requested", id);
                }
            }
            0x0c => failure(
                &mut remote,
                "遥控器拒绝打开麦克风，请松开语音键后重试".into(),
            ),
            _ => (),
        },
        4 if !remote.sessions.ignoring => {
            if let Some(session) = remote.sessions.current.as_mut() {
                match session.feed(data) {
                    Ok(level) => {
                        let _ = app.emit("mic-level", level);
                    }
                    Err(error) => failure(&mut remote, error),
                }
            }
        }
        5 => failure(&mut remote, String::from_utf8_lossy(data).into_owned()),
        _ => (),
    }
    if remote.status != old_status {
        eprintln!("remote microphone: {}", remote.status);
        let _ = app.emit_to("main", "remote-status", &remote.status);
    }
}

pub fn discard(id: u64) {
    if let Some(mut remote) = REMOTE.get().and_then(|remote| remote.lock().ok()) {
        if remote.sessions.discard(id) {
            unsafe {
                ot_remote_close();
            }
        }
    }
}

pub struct Capture {
    id: u64,
    ended: mpsc::Receiver<()>,
}
impl Capture {
    pub fn open(
        id: u64,
        run: Arc<TranscriptionRun>,
        audio: Arc<Mutex<Audio>>,
        ready: mpsc::Sender<Result<(), String>>,
    ) -> Result<Self, String> {
        let mut remote = REMOTE
            .get()
            .ok_or("遥控器监听未初始化")?
            .lock()
            .map_err(|e| e.to_string())?;
        if !remote.enabled || !remote.ready {
            return Err(remote.status.clone());
        }
        let session = remote
            .sessions
            .current
            .as_mut()
            .filter(|session| session.id == id)
            .ok_or("遥控器录音已取消或被新会话替代")?;
        let ended = session.attach(run, audio)?;
        let _ = ready.send(Ok(()));
        Ok(Self { id, ended })
    }
    pub fn finish(&self) -> Result<(), String> {
        unsafe {
            ot_remote_close();
        }
        self.ended
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| "未收到遥控器录音结束确认，已保留收到的音频".into())
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        discard(self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_real_capabilities_and_rejects_unknown_formats() {
        assert!(capabilities(&[11, 1, 0, 2, 3, 0, 120, 0, 0]).is_ok());
        assert!(capabilities(&[11, 1, 0, 0, 3, 0, 120, 0, 0]).is_ok());
        for data in [
            vec![11, 1],
            vec![11, 0, 4, 2, 3, 0, 120],
            vec![11, 1, 0, 1, 3, 0, 120],
            vec![11, 1, 0, 2, 3, 0, 0],
        ] {
            assert!(capabilities(&data).is_err());
        }
    }
    #[test]
    fn decodes_high_nibble_first_preserves_state_and_clamps() {
        let mut decoder = Decoder::default();
        assert_eq!(decoder.decode(&[0x17, 0xf0]), [1, 12, -18, -14]);
        let data = [0x77, 0x77, 0xff, 0x17];
        let mut whole = Decoder::default();
        let mut split = Decoder::default();
        let first = split.decode(&data[..2]);
        assert_eq!(
            whole.decode(&data),
            [first, split.decode(&data[2..])].concat()
        );
        assert_eq!(
            Decoder::default().decode(&[0x77; 100]).last(),
            Some(&i16::MAX)
        );
        assert_eq!(
            Decoder::default().decode(&[0xff; 100]).last(),
            Some(&i16::MIN)
        );
    }

    #[test]
    fn decoded_remote_frames_share_bounded_pcm_and_wav_with_streaming() {
        let mut decoder = Decoder::default();
        let mut audio = Audio::new(16000, 1, 2044).unwrap();
        let mut live = Vec::new();
        for frame in [
            [0; 120],
            [0; 120],
            [0x17; 120],
            [0x77; 120],
            [0xff; 120],
            [0; 120],
            [0; 120],
        ] {
            audio.push(decoder.decode(&frame).into_iter());
            live.extend(audio.pending(false));
        }
        assert!(audio.full());
        live.extend(audio.pending(true));
        let wav = audio.finish().unwrap();
        assert!(wav.len() <= 2044);
        let mut reader = hound::WavReader::new(std::io::Cursor::new(wav)).unwrap();
        assert_eq!(reader.spec().sample_rate, 16000);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(
            reader
                .samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            live
        );
    }

    #[test]
    fn atvv_stop_during_health_check_preserves_complete_audio_and_eof() {
        let mut sessions = Sessions::default();
        assert!(matches!(sessions.start(7, false).unwrap(), Start::New(1)));
        let head = sessions.current.as_mut().unwrap();
        head.feed(&[0x17; 120]).unwrap();
        assert!(matches!(sessions.start(7, true).unwrap(), Start::Duplicate));
        sessions
            .current
            .as_mut()
            .unwrap()
            .feed(&[0x77; 120])
            .unwrap();
        let expected = sessions.current.as_ref().unwrap().pending.clone();
        assert_eq!(sessions.stop(), Some(1));
        assert_eq!(sessions.stop(), None);
        let audio = Arc::new(Mutex::new(Audio::new(16000, 1, 4096).unwrap()));
        let ended = sessions
            .current
            .as_mut()
            .unwrap()
            .attach(Arc::new(TranscriptionRun::new(1)), audio.clone())
            .unwrap();
        assert_eq!(ended.try_recv(), Ok(()));
        assert!(sessions.current.as_ref().unwrap().pending.is_empty());
        sessions.discard(1);
        let wav = Arc::try_unwrap(audio)
            .ok()
            .unwrap()
            .into_inner()
            .unwrap()
            .finish()
            .unwrap();
        assert_eq!(
            hound::WavReader::new(std::io::Cursor::new(wav))
                .unwrap()
                .samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            expected
        );
    }

    #[test]
    fn cancelled_and_ignored_streams_cannot_reach_another_session() {
        let mut sessions = Sessions::default();
        assert!(matches!(sessions.start(4, false).unwrap(), Start::New(1)));
        sessions
            .current
            .as_mut()
            .unwrap()
            .feed(&[0x77; 120])
            .unwrap();
        assert!(sessions.discard(1));
        assert!(sessions.current.is_none());
        assert!(matches!(sessions.start(5, false).unwrap(), Start::Ignored));
        assert_eq!(sessions.stop(), None);
        assert!(matches!(sessions.start(5, false).unwrap(), Start::New(2)));
        assert!(!sessions.discard(1));
        assert_eq!(sessions.current.as_ref().unwrap().id, 2);
        sessions.stop();
        assert!(matches!(sessions.start(6, true).unwrap(), Start::Ignored));
        assert_eq!(sessions.stop(), None);
        assert_eq!(sessions.current.as_ref().unwrap().id, 2);
    }
}

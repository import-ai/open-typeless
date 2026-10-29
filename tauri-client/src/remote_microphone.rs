//! Xiaomi firmware 2671: device-specific voice button and ATVV v1.0 audio.
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
    id: u64,
    run: Arc<TranscriptionRun>,
    audio: Arc<Mutex<Audio>>,
    ready: Option<mpsc::Sender<Result<(), String>>>,
    ended: mpsc::Sender<()>,
}
impl Sink {
    fn start(&mut self, pending: Vec<i16>) {
        self.audio.lock().unwrap().push(pending.into_iter());
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(Ok(()));
        }
    }
}
struct Remote {
    app: AppHandle,
    enabled: bool,
    ready: bool,
    held: bool,
    busy: bool,
    press: u64,
    status: String,
    decoder: Decoder,
    streaming: bool,
    pending: Vec<i16>,
    sink: Option<Sink>,
}
static REMOTE: OnceLock<Mutex<Remote>> = OnceLock::new();

extern "C" {
    fn ot_remote_enable(callback: extern "C" fn(u32, *const u8, usize), enabled: bool);
    fn ot_remote_open();
    fn ot_remote_close();
    fn ot_remote_reset();
}

pub fn install(app: &AppHandle) {
    let _ = REMOTE.set(Mutex::new(Remote {
        app: app.clone(),
        enabled: false,
        ready: false,
        held: false,
        busy: false,
        press: 0,
        status: "小米遥控器未启用".into(),
        decoder: Decoder::default(),
        streaming: false,
        pending: Vec::new(),
        sink: None,
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
    remote.held = false;
    remote.busy = false;
    remote.streaming = false;
    remote.pending.clear();
    if let Some(sink) = remote.sink.as_ref() {
        sink.run.cancel();
    }
    remote.sink = None;
    remote.status = if enabled {
        "正在连接小米遥控器…"
    } else {
        "小米遥控器未启用"
    }
    .into();
    // The bridge always dispatches to the main queue, never calls back under this lock.
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
    remote.held = false;
    remote.busy = false;
    remote.streaming = false;
    remote.pending.clear();
    if let Some(sink) = remote.sink.as_mut() {
        *sink.run.capture_error.lock().unwrap() = Some(message.clone());
        if let Some(ready) = sink.ready.take() {
            let _ = ready.send(Err(message));
        }
        let _ = sink.ended.send(());
        let _ = remote.app.emit_to("main", "remote-stop-requested", sink.id);
    }
    unsafe {
        ot_remote_reset();
    }
}

extern "C" fn receive(kind: u32, bytes: *const u8, length: usize) {
    // All native callbacks are synchronous; their payload is copied/decoded before returning.
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
        2 => {
            let down = data[0] != 0;
            if remote.held == down {
                return;
            }
            remote.held = down;
            if down {
                remote.press = remote.press.wrapping_add(1);
                let suspended = remote
                    .app
                    .state::<crate::AppState>()
                    .shortcut_capturing
                    .lock()
                    .map(|v| *v)
                    .unwrap_or(true);
                if !suspended {
                    let _ = remote
                        .app
                        .emit_to("main", "remote-start-requested", remote.press);
                }
            } else {
                let _ = remote
                    .app
                    .emit_to("main", "remote-stop-requested", remote.press);
            }
        }
        3 => match data[0] {
            0x0b => match capabilities(data) {
                Ok(()) => {
                    remote.ready = true;
                    remote.status = "小米遥控器已就绪 · 按住语音键说话，松开后识别".into();
                }
                Err(error) => {
                    remote.ready = false;
                    failure(&mut remote, error);
                }
            },
            4 => {
                if data.len() < 4 || data[2] != 2 {
                    failure(&mut remote, "遥控器返回了不支持的录音格式".into());
                } else {
                    remote.decoder = Decoder::default();
                    remote.streaming = true;
                    remote.pending.clear();
                    if let Some(sink) = remote.sink.as_mut() {
                        sink.start(Vec::new());
                    }
                }
            }
            0x0a => {
                if data.len() != 7 || data[1] != 2 || data[6] > 88 {
                    failure(&mut remote, "遥控器音频同步数据无效".into());
                } else {
                    remote.decoder = Decoder {
                        predictor: i16::from_be_bytes([data[4], data[5]]) as i32,
                        index: data[6] as usize,
                    };
                }
            }
            0 => {
                remote.busy = false;
                remote.streaming = false;
                remote.pending.clear();
                if let Some(sink) = remote.sink.as_mut() {
                    if let Some(ready) = sink.ready.take() {
                        let _ = ready.send(Err("录音过短，未收到遥控器音频".into()));
                    }
                    let _ = sink.ended.send(());
                    let _ = app.emit_to("main", "remote-stop-requested", sink.id);
                }
            }
            0x0c => failure(
                &mut remote,
                "遥控器拒绝打开麦克风，请松开语音键后重试".into(),
            ),
            _ => {}
        },
        4 => {
            if !remote.streaming
                || remote
                    .sink
                    .as_ref()
                    .is_some_and(|sink| sink.run.check().is_err())
            {
                return;
            }
            if data.len() != 120 {
                failure(&mut remote, "遥控器音频帧长度无效".into());
            } else {
                let samples = remote.decoder.decode(data);
                let level = samples
                    .iter()
                    .map(|sample| (*sample as f64).abs())
                    .sum::<f64>()
                    / (samples.len() as f64 * 32768.0);
                if let Some(sink) = remote.sink.as_ref() {
                    sink.audio.lock().unwrap().push(samples.into_iter());
                } else if remote.pending.len() + samples.len()
                    <= 16000 * crate::recording::MAX_RECORDING_SECONDS as usize
                {
                    // The remote can start before HID; retain audio until health checks finish.
                    remote.pending.extend(samples);
                } else {
                    failure(&mut remote, "遥控器音频缓冲已满，请松开语音键后重试".into());
                }
                let _ = remote.app.emit("mic-level", level);
            }
        }
        5 => failure(&mut remote, String::from_utf8_lossy(data).into_owned()),
        _ => return,
    }
    if remote.status != old_status {
        eprintln!("remote microphone: {}", remote.status);
        let _ = remote.app.emit_to("main", "remote-status", &remote.status);
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
        if !remote.held || remote.press != id {
            return Err("语音键已松开，请按住语音键后说话".into());
        }
        if remote.sink.is_some() || remote.busy {
            return Err("遥控器正在结束上次录音，请松开后重试".into());
        }
        let (ended, receive) = mpsc::channel();
        let mut sink = Sink {
            id,
            run,
            audio,
            ready: Some(ready),
            ended,
        };
        if remote.streaming {
            sink.start(std::mem::take(&mut remote.pending));
        }
        remote.sink = Some(sink);
        remote.busy = true;
        unsafe {
            ot_remote_open();
        }
        Ok(Self { id, ended: receive })
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
        if let Some(mut remote) = REMOTE.get().and_then(|remote| remote.lock().ok()) {
            if remote.sink.as_ref().is_some_and(|sink| sink.id == self.id) {
                unsafe {
                    ot_remote_close();
                }
                remote.sink = None;
            }
        }
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
    fn audio_started_before_capture_preserves_the_head_and_reports_ready_once() {
        let audio = Arc::new(Mutex::new(Audio::new(16000, 1, 4096).unwrap()));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (ended, _) = mpsc::channel();
        let mut sink = Sink {
            id: 1,
            run: Arc::new(TranscriptionRun::new(1)),
            audio: audio.clone(),
            ready: Some(ready_tx),
            ended,
        };
        let mut decoder = Decoder::default();
        let head = decoder.decode(&[0x17; 120]);
        sink.start(head.clone());
        sink.start(Vec::new());
        assert_eq!(ready_rx.try_recv(), Ok(Ok(())));
        assert!(ready_rx.try_recv().is_err());
        let tail = decoder.decode(&[0x77; 120]);
        audio.lock().unwrap().push(tail.iter().copied());
        let wav = Arc::try_unwrap({
            drop(sink);
            audio
        })
        .ok()
        .unwrap()
        .into_inner()
        .unwrap()
        .finish()
        .unwrap();
        let samples = hound::WavReader::new(std::io::Cursor::new(wav))
            .unwrap()
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(samples, [head, tail].concat());
    }
}

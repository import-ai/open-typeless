use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

struct Recorder {
    session: Option<RecordingSession>,
}
struct RecordingSession {
    stop: mpsc::Sender<()>,
    done: mpsc::Receiver<Result<PathBuf, String>>,
}
struct AppState {
    recorder: Mutex<Recorder>,
    server_url: Mutex<String>,
    shortcut: Mutex<String>,
}
#[derive(Deserialize)]
struct Recognition {
    raw_text: String,
}
#[derive(Serialize)]
struct Settings {
    shortcut: String,
    server_url: String,
}

/// Remove the leading capture delay while retaining a short amount of context
/// before speech. Audio is interleaved by channel.
fn trim_leading_silence(
    samples: &[i16],
    channels: usize,
    sample_rate: u32,
) -> Result<Vec<i16>, String> {
    if channels == 0 || sample_rate == 0 || samples.is_empty() {
        return Err("未检测到有效音频".into());
    }

    let window_frames = (sample_rate / 100).max(1) as usize; // 10 ms
    let total_frames = samples.len() / channels;
    let threshold = 32768.0_f64 * 10_f64.powf(-42.0 / 20.0);
    let mut first_active_window = None;
    let window_count = (total_frames + window_frames - 1) / window_frames;

    for window in 0..window_count.saturating_sub(1) {
        let start_frame = window * window_frames;
        let end_frame = ((window + 1) * window_frames).min(total_frames);
        let next_start = end_frame;
        let next_end = ((window + 2) * window_frames).min(total_frames);
        if rms(samples, channels, start_frame, end_frame) > threshold
            && rms(samples, channels, next_start, next_end) > threshold
        {
            first_active_window = Some(window);
            break;
        }
    }

    let speech_frame =
        first_active_window.ok_or_else(|| "未检测到有效音频".to_string())? * window_frames;
    let pre_roll_frames = (sample_rate / 10) as usize; // 100 ms
    let start_frame = speech_frame
        .saturating_sub(pre_roll_frames)
        .min(total_frames);
    Ok(samples[start_frame * channels..].to_vec())
}

fn rms(samples: &[i16], channels: usize, start_frame: usize, end_frame: usize) -> f64 {
    if end_frame <= start_frame {
        return 0.0;
    }
    let start = start_frame * channels;
    let end = (end_frame * channels).min(samples.len());
    let mut sum = 0.0;
    for sample in &samples[start..end] {
        let value = *sample as f64;
        sum += value * value;
    }
    (sum / (end - start) as f64).sqrt()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            recorder: Mutex::new(Recorder { session: None }),
            server_url: Mutex::new("http://127.0.0.1:8080".into()),
            shortcut: Mutex::new(default_shortcut_name().into()),
        })
        .setup(|app| {
            bind_shortcut(app.handle(), parse_shortcut(default_shortcut_name())?)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            transcribe_file,
            set_server_url,
            get_settings,
            set_shortcut
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenTypeless");
}

fn default_shortcut_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Command+Shift+Space"
    } else {
        "Control+Shift+Space"
    }
}

fn parse_shortcut(shortcut_text: &str) -> Result<Shortcut, String> {
    match shortcut_text.trim().to_ascii_uppercase().as_str() {
        _ => shortcut_text
            .parse()
            .map_err(|e| format!("快捷键格式无效: {e}")),
    }
}

fn bind_shortcut(app: &AppHandle, shortcut: Shortcut) -> Result<(), String> {
    let handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                if let Err(error) = toggle_recording(&handle) {
                    eprintln!("recording shortcut failed: {error}");
                    let _ = handle.emit("recording-error", error);
                }
            }
        })
        .map_err(|e| e.to_string())
}

fn toggle_recording(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let recording = state
        .recorder
        .lock()
        .map_err(|e| e.to_string())?
        .session
        .is_some();
    if recording {
        let path = stop_inner(&state)?;
        app.emit("recording-stopped", path.to_string_lossy().to_string())
            .map_err(|e| e.to_string())?;
    } else {
        app.emit("recording-starting", ())
            .map_err(|e| e.to_string())?;
        start_inner(&state)?;
        app.emit("recording-started", ())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn start_recording(state: State<'_, AppState>) -> Result<(), String> {
    start_inner(&state)
}
fn start_inner(state: &AppState) -> Result<(), String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (capture_ready_tx, capture_ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let host = cpal::default_host();
            let device = host.default_input_device().ok_or("没有可用的麦克风")?;
            let supported = device.default_input_config().map_err(|e| e.to_string())?;
            let sample_rate = supported.sample_rate().0;
            let channels = supported.channels();
            let samples = Arc::new(Mutex::new(Vec::new()));
            let sink = samples.clone();
            let capture_ready_once = Arc::new(AtomicBool::new(false));
            let err_fn = |e| eprintln!("audio input error: {e}");
            let stream = match supported.sample_format() {
                cpal::SampleFormat::I16 => {
                    let ready = capture_ready_tx.clone();
                    let once = capture_ready_once.clone();
                    device.build_input_stream(
                        &supported.config(),
                        move |d: &[i16], _| {
                            if d.iter().any(|sample| *sample != 0)
                                && !once.swap(true, Ordering::AcqRel)
                            {
                                let _ = ready.send(());
                            }
                            sink.lock().unwrap().extend_from_slice(d)
                        },
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::U16 => {
                    let ready = capture_ready_tx.clone();
                    let once = capture_ready_once.clone();
                    device.build_input_stream(
                        &supported.config(),
                        move |d: &[u16], _| {
                            if d.iter().any(|sample| *sample != 32768)
                                && !once.swap(true, Ordering::AcqRel)
                            {
                                let _ = ready.send(());
                            }
                            sink.lock()
                                .unwrap()
                                .extend(d.iter().map(|x| (*x as i32 - 32768) as i16))
                        },
                        err_fn,
                        None,
                    )
                }
                cpal::SampleFormat::F32 => {
                    let ready = capture_ready_tx.clone();
                    let once = capture_ready_once.clone();
                    device.build_input_stream(
                        &supported.config(),
                        move |d: &[f32], _| {
                            if d.iter().any(|sample| *sample != 0.0)
                                && !once.swap(true, Ordering::AcqRel)
                            {
                                let _ = ready.send(());
                            }
                            sink.lock()
                                .unwrap()
                                .extend(d.iter().map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16))
                        },
                        err_fn,
                        None,
                    )
                }
                _ => return Err("不支持的麦克风采样格式".into()),
            }
            .map_err(|e| e.to_string())?;
            stream.play().map_err(|e| e.to_string())?;
            capture_ready_rx
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| "麦克风未及时就绪，请检查麦克风权限或输入设备".to_string())?;
            ready_tx.send(Ok(())).map_err(|e| e.to_string())?;
            stop_rx.recv().map_err(|e| e.to_string())?;
            // The input device can have one callback already queued when the
            // user releases the hotkey/button. Keep the stream alive briefly
            // so the tail of the utterance reaches `samples` before closing.
            std::thread::sleep(Duration::from_millis(200));
            drop(stream);
            let captured = samples.lock().map_err(|e| e.to_string())?.clone();
            let samples = trim_leading_silence(&captured, channels as usize, sample_rate)?;
            let path =
                std::env::temp_dir().join(format!("open-typeless-{}.wav", std::process::id()));
            let spec = hound::WavSpec {
                channels,
                sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
            for sample in samples {
                writer.write_sample(sample).map_err(|e| e.to_string())?;
            }
            writer.finalize().map_err(|e| e.to_string())?;
            done_tx.send(Ok(path)).map_err(|e| e.to_string())?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = ready_tx.send(Err(error.clone()));
            let _ = done_tx.send(Err(error));
        }
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())??;
    state.recorder.lock().map_err(|e| e.to_string())?.session = Some(RecordingSession {
        stop: stop_tx,
        done: done_rx,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::trim_leading_silence;

    #[test]
    fn trims_leading_silence_and_keeps_pre_roll() {
        let sample_rate = 48_000;
        let channels = 2;
        let mut samples = vec![0_i16; sample_rate as usize * channels];
        samples.extend(std::iter::repeat_n(
            10_000_i16,
            sample_rate as usize * channels,
        ));

        let trimmed = trim_leading_silence(&samples, channels, sample_rate).unwrap();
        assert_eq!(
            trimmed.len(),
            (sample_rate as usize + sample_rate as usize / 10) * channels
        );
        assert!(trimmed[..sample_rate as usize / 10 * channels]
            .iter()
            .all(|sample| *sample == 0));
        assert!(trimmed[sample_rate as usize / 10 * channels..]
            .iter()
            .all(|sample| *sample == 10_000));
    }

    #[test]
    fn rejects_audio_without_two_active_windows() {
        let sample_rate = 48_000;
        let samples = vec![0_i16; sample_rate as usize * 2];
        assert_eq!(
            trim_leading_silence(&samples, 1, sample_rate),
            Err("未检测到有效音频".into())
        );
    }
}

#[tauri::command]
fn stop_recording(state: State<'_, AppState>) -> Result<String, String> {
    Ok(stop_inner(&state)?.to_string_lossy().to_string())
}
fn stop_inner(state: &AppState) -> Result<PathBuf, String> {
    let session = state
        .recorder
        .lock()
        .map_err(|e| e.to_string())?
        .session
        .take()
        .ok_or("当前没有录音")?;
    session.stop.send(()).map_err(|e| e.to_string())?;
    session
        .done
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn transcribe_file(app: AppHandle, path: String) -> Result<String, String> {
    let url = app
        .state::<AppState>()
        .server_url
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        + "/v1/recognitions";
    let data = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
    let part = reqwest::multipart::Part::bytes(data)
        .file_name("recording.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .part("audio", part)
        .text("language", "auto");
    let result: Recognition = reqwest::Client::new()
        .post(url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let _ = tokio::fs::remove_file(&path).await;
    let text = result.raw_text.clone();
    let (paste_tx, paste_rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let outcome = (|| -> Result<(), String> {
            let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
            clipboard.set_text(text).map_err(|e| e.to_string())?;
            let mut enigo =
                enigo::Enigo::new(&enigo::Settings::default()).map_err(|e| e.to_string())?;
            use enigo::{Direction, Key, Keyboard};
            let modifier = if cfg!(target_os = "macos") {
                Key::Meta
            } else {
                Key::Control
            };
            enigo
                .key(modifier, Direction::Press)
                .map_err(|e| e.to_string())?;
            enigo
                .key(Key::Unicode('v'), Direction::Click)
                .map_err(|e| e.to_string())?;
            enigo
                .key(modifier, Direction::Release)
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        let _ = paste_tx.send(outcome);
    })
    .map_err(|e| e.to_string())?;
    paste_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())??;
    Ok(result.raw_text)
}

#[tauri::command]
fn set_server_url(state: State<'_, AppState>, url: String) -> Result<(), String> {
    *state.server_url.lock().map_err(|e| e.to_string())? = url.trim_end_matches('/').to_string();
    Ok(())
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(Settings {
        shortcut: state.shortcut.lock().map_err(|e| e.to_string())?.clone(),
        server_url: state.server_url.lock().map_err(|e| e.to_string())?.clone(),
    })
}

#[tauri::command]
fn set_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
) -> Result<(), String> {
    let shortcut = shortcut.trim().to_string();
    if shortcut.is_empty() {
        return Err("快捷键不能为空".into());
    }
    let new_shortcut = parse_shortcut(&shortcut)?;
    let mut current = state.shortcut.lock().map_err(|e| e.to_string())?;
    if *current == shortcut {
        return Ok(());
    }
    let old = parse_shortcut(&current)?;
    app.global_shortcut()
        .unregister(old)
        .map_err(|e| e.to_string())?;
    if let Err(error) = bind_shortcut(&app, new_shortcut) {
        let _ = bind_shortcut(&app, old);
        return Err(error);
    }
    *current = shortcut;
    Ok(())
}

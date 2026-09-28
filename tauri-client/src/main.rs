use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

struct Recorder {
    stream: Option<cpal::Stream>,
    samples: Arc<Mutex<Vec<i16>>>,
    sample_rate: u32,
    channels: u16,
}
struct AppState {
    recorder: Mutex<Recorder>,
    server_url: Mutex<String>,
}
// cpal's CoreAudio stream is guarded by `recorder`; Tauri requires managed state
// to be Send + Sync even though the stream is only accessed through this mutex.
unsafe impl Send for Recorder {}
unsafe impl Sync for Recorder {}
#[derive(Deserialize)]
struct Recognition {
    raw_text: String,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            recorder: Mutex::new(Recorder {
                stream: None,
                samples: Arc::new(Mutex::new(Vec::new())),
                sample_rate: 16000,
                channels: 1,
            }),
            server_url: Mutex::new("http://127.0.0.1:8080".into()),
        })
        .setup(|app| {
            let shortcut: Shortcut = "CommandOrControl+Shift+Space".parse().unwrap();
            let handle = app.handle().clone();
            app.global_shortcut()
                .on_shortcut(shortcut, move |_app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        if let Err(error) = toggle_recording(&handle) {
                            eprintln!("recording shortcut failed: {error}");
                            let _ = handle.emit("recording-error", error);
                        }
                    }
                })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            transcribe_file,
            set_server_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenTypeless");
}

fn toggle_recording(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let recording = state
        .recorder
        .lock()
        .map_err(|e| e.to_string())?
        .stream
        .is_some();
    if recording {
        let path = stop_inner(&state)?;
        app.emit("recording-stopped", path.to_string_lossy().to_string())
            .map_err(|e| e.to_string())?;
    } else {
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
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("没有可用的麦克风")?;
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels();
    let samples = Arc::new(Mutex::new(Vec::new()));
    let sink = samples.clone();
    let err_fn = |e| eprintln!("audio input error: {e}");
    let stream = match supported.sample_format() {
        cpal::SampleFormat::I16 => device.build_input_stream(
            &supported.config(),
            move |d: &[i16], _| sink.lock().unwrap().extend_from_slice(d),
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &supported.config(),
            move |d: &[u16], _| {
                sink.lock()
                    .unwrap()
                    .extend(d.iter().map(|x| (*x as i32 - 32768) as i16))
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::F32 => device.build_input_stream(
            &supported.config(),
            move |d: &[f32], _| {
                sink.lock()
                    .unwrap()
                    .extend(d.iter().map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16))
            },
            err_fn,
            None,
        ),
        _ => return Err("不支持的麦克风采样格式".into()),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    let mut rec = state.recorder.lock().map_err(|e| e.to_string())?;
    rec.stream = Some(stream);
    rec.samples = samples;
    rec.sample_rate = sample_rate;
    rec.channels = channels;
    Ok(())
}

#[tauri::command]
fn stop_recording(state: State<'_, AppState>) -> Result<String, String> {
    Ok(stop_inner(&state)?.to_string_lossy().to_string())
}
fn stop_inner(state: &AppState) -> Result<PathBuf, String> {
    let mut rec = state.recorder.lock().map_err(|e| e.to_string())?;
    let stream = rec.stream.take();
    drop(stream);
    let samples = std::mem::replace(&mut rec.samples, Arc::new(Mutex::new(Vec::new())))
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    if samples.is_empty() {
        return Err("没有录到音频".into());
    }
    let path = std::env::temp_dir().join(format!("open-typeless-{}.wav", std::process::id()));
    let spec = hound::WavSpec {
        channels: rec.channels,
        sample_rate: rec.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
    for s in samples {
        writer.write_sample(s).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(path)
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
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_text(result.raw_text.clone())
        .map_err(|e| e.to_string())?;
    let mut enigo = enigo::Enigo::new(&enigo::Settings::default()).map_err(|e| e.to_string())?;
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
    Ok(result.raw_text)
}

#[tauri::command]
fn set_server_url(state: State<'_, AppState>, url: String) -> Result<(), String> {
    *state.server_url.lock().map_err(|e| e.to_string())? = url.trim_end_matches('/').to_string();
    Ok(())
}

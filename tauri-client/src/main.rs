mod modifier_shortcut;
mod settings_file;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

struct Recorder {
    session: Option<RecordingSession>,
}
struct RecordingSession {
    run: Arc<TranscriptionRun>,
    stop: mpsc::Sender<()>,
    done: mpsc::Receiver<Result<PathBuf, String>>,
}
struct AppState {
    recorder: Mutex<Recorder>,
    active: Mutex<Option<Arc<TranscriptionRun>>>,
    next_run: AtomicU64,
    server_url: Mutex<String>,
    shortcut: Mutex<String>,
    shortcut_capturing: Mutex<bool>,
    settings_warning: Mutex<Option<String>>,
}
const CANCELLED: &str = "已取消本次识别";

struct TranscriptionRun {
    id: u64,
    cancelled: AtomicBool,
    notification: tokio::sync::Notify,
    capture_stop: Mutex<Option<mpsc::Sender<()>>>,
}
impl TranscriptionRun {
    fn new(id: u64) -> Self {
        Self {
            id,
            cancelled: AtomicBool::new(false),
            notification: tokio::sync::Notify::new(),
            capture_stop: Mutex::new(None),
        }
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notification.notify_one();
        if let Ok(stop) = self.capture_stop.lock() {
            if let Some(stop) = stop.as_ref() {
                let _ = stop.send(());
            }
        }
    }
    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::SeqCst) {
            Err(CANCELLED.into())
        } else {
            Ok(())
        }
    }
    async fn cancellation(&self) {
        if self.check().is_ok() {
            self.notification.notified().await;
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct RecordingFile {
    path: String,
    run_id: u64,
}

#[derive(Deserialize)]
struct Recognition {
    raw_text: String,
}
#[derive(Serialize)]
struct Settings {
    shortcut: String,
    server_url: String,
    shortcut_warning: Option<String>,
    developer_options: bool,
    settings_warning: Option<String>,
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
            active: Mutex::new(None),
            next_run: AtomicU64::new(1),
            server_url: Mutex::new(String::new()),
            shortcut: Mutex::new(default_shortcut_name().into()),
            shortcut_capturing: Mutex::new(false),
            settings_warning: Mutex::new(None),
        })
        .setup(|app| {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if let Err(error) = modifier_shortcut::install(app.handle()) {
                eprintln!("{error}");
            }
            restore_settings(app.handle())?;
            position_pill(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(
                    event,
                    tauri::WindowEvent::Focused(false) | tauri::WindowEvent::Destroyed
                )
            {
                let app = window.app_handle();
                if let Err(error) = set_shortcut_capture(app.clone(), app.state(), false) {
                    eprintln!("恢复快捷键失败: {error}");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            cancel_recording,
            dismiss_pill,
            debug_pill_preview,
            transcribe_file,
            set_server_url,
            check_server_connection,
            get_settings,
            set_shortcut,
            set_shortcut_capture
        ])
        .run(tauri::generate_context!())
        .expect("error while running Open Typeless");
}

fn default_shortcut_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "RCommand"
    } else if cfg!(target_os = "windows") {
        "RControl"
    } else {
        "Control+Shift+Space"
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("settings.json"))
        .map_err(|e| e.to_string())
}

fn restore_settings(app: &AppHandle) -> Result<(), String> {
    let mut warnings = Vec::new();
    let mut saved = match settings_path(app).and_then(|path| settings_file::load(&path)) {
        Ok(saved) => saved,
        Err(error) => {
            warnings.push(error);
            settings_file::SavedSettings::default()
        }
    };
    saved.server_url = match normalize_server_url(&saved.server_url) {
        Ok(url) => url,
        Err(error) => {
            warnings.push(format!("保存的后端地址无效: {error}"));
            String::new()
        }
    };
    saved.shortcut = saved.shortcut.trim().to_owned();
    if let Err(error) = bind_named_shortcut(app, &saved.shortcut) {
        warnings.push(format!(
            "无法恢复快捷键 {}: {error}，已改用默认快捷键",
            saved.shortcut
        ));
        saved.shortcut = default_shortcut_name().into();
        bind_named_shortcut(app, &saved.shortcut)?;
    }
    let state = app.state::<AppState>();
    *state.shortcut.lock().map_err(|e| e.to_string())? = saved.shortcut;
    *state.server_url.lock().map_err(|e| e.to_string())? = saved.server_url;
    *state.settings_warning.lock().map_err(|e| e.to_string())? =
        (!warnings.is_empty()).then(|| warnings.join("；"));
    Ok(())
}

fn persist_settings(app: &AppHandle, shortcut: &str, server_url: &str) -> Result<(), String> {
    settings_file::save(
        &settings_path(app)?,
        &settings_file::SavedSettings {
            shortcut: shortcut.into(),
            server_url: server_url.into(),
        },
    )
}

fn is_modifier(name: &str) -> bool {
    modifier_shortcut::is_modifier(name)
}

fn bind_named_shortcut(app: &AppHandle, name: &str) -> Result<(), String> {
    if is_modifier(name) {
        modifier_shortcut::configure(Some(name))
    } else {
        bind_shortcut(app, parse_shortcut(name)?)
    }
}

fn unbind_named_shortcut(app: &AppHandle, name: &str) -> Result<(), String> {
    if is_modifier(name) {
        modifier_shortcut::configure(None)
    } else {
        app.global_shortcut()
            .unregister(parse_shortcut(name)?)
            .map_err(|e| e.to_string())
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
                // Esc remains cancellation while the pill is visible, even when
                // the user chose it as the idle activation shortcut.
                if shortcut == parse_shortcut("Escape").unwrap()
                    && handle
                        .get_webview_window("pill")
                        .is_some_and(|w| w.is_visible().unwrap_or(false))
                {
                    let _ = cancel_active(&handle);
                    return;
                }
                let _ = handle.emit_to("main", "toggle-requested", ());
            }
        })
        .map_err(|e| e.to_string())
}

fn with_current_run<T>(
    active: &Mutex<Option<Arc<TranscriptionRun>>>,
    run: &TranscriptionRun,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let current = active.lock().map_err(|e| e.to_string())?;
    if current.as_ref().is_none_or(|current| current.id != run.id) {
        return Err(CANCELLED.into());
    }
    run.check()?;
    action()
}
fn recording_path(id: u64) -> PathBuf {
    std::env::temp_dir().join(format!("open-typeless-{}-{}.wav", std::process::id(), id))
}

async fn finish_run(app: &AppHandle, id: u64) {
    let finish_app = app.clone();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    if app
        .run_on_main_thread(move || {
            let state = finish_app.state::<AppState>();
            let mut active = state.active.lock().unwrap();
            if active.as_ref().is_some_and(|run| run.id == id) {
                *active = None;
                hide_pill(&finish_app);
            }
            let _ = done_tx.send(());
        })
        .is_ok()
    {
        let _ = done_rx.await;
    }
}

#[tauri::command]
async fn start_recording(app: AppHandle) -> Result<(), String> {
    if app
        .state::<AppState>()
        .server_url
        .lock()
        .map_err(|e| e.to_string())?
        .is_empty()
    {
        return Err("后端地址未设置".into());
    }
    let run = {
        let state = app.state::<AppState>();
        let mut active = state.active.lock().map_err(|e| e.to_string())?;
        if active.is_some() {
            return Err("已有录音或识别进行中".into());
        }
        let run = Arc::new(TranscriptionRun::new(
            state.next_run.fetch_add(1, Ordering::SeqCst),
        ));
        *active = Some(run.clone());
        run
    };
    let result = async {
        // Native shortcut registration must run on the UI thread. Never hold
        // the active-run lock on a worker while waiting for the UI thread.
        let show_app = app.clone();
        let show_run = run.clone();
        let (shown_tx, shown_rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let result = with_current_run(&show_app.state::<AppState>().active, &show_run, || {
                show_pill(&show_app)?;
                show_app
                    .emit("recording-starting", ())
                    .map_err(|e| e.to_string())
            });
            let _ = shown_tx.send(result);
        })
        .map_err(|e| e.to_string())?;
        shown_rx.await.map_err(|e| e.to_string())??;
        let worker_app = app.clone();
        let worker_run = run.clone();
        tauri::async_runtime::spawn_blocking(move || {
            start_inner(
                &worker_app.state::<AppState>(),
                worker_app.clone(),
                worker_run,
            )
        })
        .await
        .map_err(|e| e.to_string())?
    }
    .await;
    if result.is_err() {
        run.cancel();
        finish_run(&app, run.id).await;
    }
    result
}
fn start_inner(state: &AppState, app: AppHandle, run: Arc<TranscriptionRun>) -> Result<(), String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (capture_ready_tx, capture_ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    *run.capture_stop.lock().map_err(|e| e.to_string())? = Some(stop_tx.clone());
    let capture_run = run.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            capture_run.check()?;
            let host = cpal::default_host();
            let device = match host.default_input_device() {
                Some(device) => device,
                None => {
                    let _ = app.emit("mic-state", "disconnected");
                    return Err("没有可用的麦克风".into());
                }
            };
            let supported = device.default_input_config().map_err(|e| e.to_string())?;
            let sample_rate = supported.sample_rate().0;
            let channels = supported.channels();
            let samples = Arc::new(Mutex::new(Vec::new()));
            let sink = samples.clone();
            let capture_ready_once = Arc::new(AtomicBool::new(false));
            let error_events = app.clone();
            let error_run = capture_run.clone();
            let err_fn = move |e| {
                eprintln!("audio input error: {e}");
                if error_run.check().is_ok() {
                    let _ = error_events.emit("mic-state", "disconnected");
                }
            };
            let stream = match supported.sample_format() {
                cpal::SampleFormat::I16 => {
                    let ready = capture_ready_tx.clone();
                    let once = capture_ready_once.clone();
                    let events = app.clone();
                    let callback_run = capture_run.clone();
                    device.build_input_stream(
                        &supported.config(),
                        move |d: &[i16], _| {
                            if callback_run.check().is_err() {
                                return;
                            }
                            let level = d.iter().map(|sample| (*sample as f64).abs()).sum::<f64>()
                                / (d.len().max(1) as f64)
                                / 32768.0;
                            let _ = events.emit("mic-level", level.min(1.0));
                            if d.iter().any(|sample| *sample != 0)
                                && !once.swap(true, Ordering::AcqRel)
                            {
                                let _ = ready.send(());
                                let _ = events.emit("mic-state", "ready");
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
                    let events = app.clone();
                    let callback_run = capture_run.clone();
                    device.build_input_stream(
                        &supported.config(),
                        move |d: &[u16], _| {
                            if callback_run.check().is_err() {
                                return;
                            }
                            let level = d
                                .iter()
                                .map(|sample| ((*sample as i32 - 32768).abs()) as f64)
                                .sum::<f64>()
                                / (d.len().max(1) as f64)
                                / 32768.0;
                            let _ = events.emit("mic-level", level.min(1.0));
                            if d.iter().any(|sample| *sample != 32768)
                                && !once.swap(true, Ordering::AcqRel)
                            {
                                let _ = ready.send(());
                                let _ = events.emit("mic-state", "ready");
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
                    let events = app.clone();
                    let callback_run = capture_run.clone();
                    device.build_input_stream(
                        &supported.config(),
                        move |d: &[f32], _| {
                            if callback_run.check().is_err() {
                                return;
                            }
                            let level = d.iter().map(|sample| sample.abs() as f64).sum::<f64>()
                                / (d.len().max(1) as f64);
                            let _ = events.emit("mic-level", level.min(1.0));
                            if d.iter().any(|sample| *sample != 0.0)
                                && !once.swap(true, Ordering::AcqRel)
                            {
                                let _ = ready.send(());
                                let _ = events.emit("mic-state", "ready");
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
            capture_run.check()?;
            let _ = app.emit("mic-state", "unready");
            stream.play().map_err(|e| e.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                capture_run.check()?;
                match capture_ready_rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(()) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => continue,
                    _ => return Err("麦克风未及时就绪，请检查麦克风权限和输入设备".into()),
                }
            }
            ready_tx.send(Ok(())).map_err(|e| e.to_string())?;
            stop_rx.recv().map_err(|e| e.to_string())?;
            // The input device can have one callback already queued when the
            // user releases the hotkey/button. Keep the stream alive briefly
            // so the tail of the utterance reaches `samples` before closing.
            std::thread::sleep(Duration::from_millis(200));
            drop(stream);
            capture_run.check()?;
            let captured = samples.lock().map_err(|e| e.to_string())?.clone();
            let samples = trim_leading_silence(&captured, channels as usize, sample_rate)?;
            let path = recording_path(capture_run.id);
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
            if capture_run.check().is_err() || done_tx.send(Ok(path.clone())).is_err() {
                let _ = std::fs::remove_file(path);
                return Err(CANCELLED.into());
            }
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
    let mut recorder = state.recorder.lock().map_err(|e| e.to_string())?;
    run.check()?;
    recorder.session = Some(RecordingSession {
        run,
        stop: stop_tx,
        done: done_rx,
    });
    Ok(())
}

fn pill_position(origin: (i32, i32), size: (u32, u32), scale: f64) -> (i32, i32) {
    let width = (128.0 * scale).round() as i32;
    let height = (32.0 * scale).round() as i32;
    (
        origin.0 + (size.0 as i32 - width) / 2,
        origin.1 + (size.1 as i32 - height - (128.0 * scale).round() as i32).max(0),
    )
}
fn position_pill(app: &AppHandle) -> Result<(), String> {
    if let (Some(window), Some(monitor)) = (
        app.get_webview_window("pill"),
        app.primary_monitor().map_err(|e| e.to_string())?,
    ) {
        let (x, y) = pill_position(
            (monitor.position().x, monitor.position().y),
            (monitor.size().width, monitor.size().height),
            monitor.scale_factor(),
        );
        window
            .set_position(tauri::PhysicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn show_pill(app: &AppHandle) -> Result<(), String> {
    let escape = parse_shortcut("Escape")?;
    if !app.global_shortcut().is_registered(escape) {
        let handle = app.clone();
        app.global_shortcut()
            .on_shortcut(escape, move |_, _, event| {
                if event.state == ShortcutState::Pressed {
                    let _ = cancel_active(&handle);
                }
            })
            .map_err(|e| e.to_string())?;
    }
    if let Some(window) = app.get_webview_window("pill") {
        window.show().map_err(|e| e.to_string())?;
    }
    app.emit("pill-shown", ()).map_err(|e| e.to_string())?;
    Ok(())
}
fn hide_pill(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("pill") {
        let _ = window.hide();
    }
    let shortcut_is_escape = app
        .state::<AppState>()
        .shortcut
        .lock()
        .map(|name| parse_shortcut(&name).ok() == parse_shortcut("Escape").ok())
        .unwrap_or(false);
    if !shortcut_is_escape {
        if let Ok(escape) = parse_shortcut("Escape") {
            let _ = app.global_shortcut().unregister(escape);
        }
    }
    let _ = app.emit("mic-state", "disconnected");
    let _ = app.emit("pill-hidden", ());
}

#[tauri::command]
fn dismiss_pill(app: AppHandle) {
    hide_pill(&app);
}

// Exercise the real native window lifecycle without starting a microphone or ASR.
fn developer_options_enabled() -> bool {
    cfg!(debug_assertions) && std::env::var("OPEN_TYPELESS_DEBUG").as_deref() == Ok("1")
}

#[tauri::command]
fn debug_pill_preview(app: AppHandle, mode: String) -> Result<String, String> {
    if !developer_options_enabled() {
        return Err("开发者选项仅在 tauri:debug 模式启用".into());
    }
    if app
        .state::<AppState>()
        .active
        .lock()
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err("请先结束录音和识别".into());
    }
    let window = app.get_webview_window("pill").ok_or("找不到 pill 窗口")?;
    match mode.as_str() {
        "disconnected" | "unready" | "ready" | "processing" => {
            app.emit_to("pill", "pill-preview", &mode)
                .map_err(|e| e.to_string())?;
            show_pill(&app)?;
        }
        "hidden" => hide_pill(&app),
        _ => return Err("未知预览状态".into()),
    }
    // Keep the controls usable while the always-on-top overlay is visible.
    if let Some(main) = app.get_webview_window("main") {
        main.set_focus().map_err(|e| e.to_string())?;
    }
    let size = window.inner_size().map_err(|e| e.to_string())?;
    let logical = size.to_logical::<f64>(window.scale_factor().map_err(|e| e.to_string())?);
    let status = format!("{}×{} · {}", logical.width, logical.height, mode);
    eprintln!("pill preview: {status}");
    Ok(status)
}

fn cancel_active(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut active = state.active.lock().map_err(|e| e.to_string())?;
    if let Some(run) = active.take() {
        run.cancel();
        let mut recorder = state.recorder.lock().map_err(|e| e.to_string())?;
        if recorder
            .session
            .as_ref()
            .is_some_and(|session| session.run.id == run.id)
        {
            recorder.session.take();
        }
        let _ = std::fs::remove_file(recording_path(run.id));
    }
    hide_pill(app);
    app.emit("recording-cancelled", ())
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn cancel_recording(app: AppHandle) -> Result<(), String> {
    cancel_active(&app)
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
    #[test]
    fn positions_pill_in_screen_coordinates_at_each_scale() {
        assert_eq!(super::pill_position((0, 0), (1920, 1080), 1.0), (896, 920));
        assert_eq!(
            super::pill_position((0, 0), (2880, 1800), 2.0),
            (1312, 1480)
        );
        assert_eq!(
            super::pill_position((-1920, -100), (1920, 1080), 1.0),
            (-1024, 820)
        );
    }

    #[test]
    fn cancelled_and_superseded_results_cannot_commit() {
        use super::*;
        let old = Arc::new(TranscriptionRun::new(1));
        let fresh = Arc::new(TranscriptionRun::new(2));
        let active = Mutex::new(Some(old.clone()));
        old.cancel();
        assert_eq!(
            with_current_run(&active, &old, || panic!("cancelled result pasted")),
            Err::<(), _>(CANCELLED.into())
        );
        *active.lock().unwrap() = Some(fresh.clone());
        assert_eq!(
            with_current_run(&active, &old, || panic!("old result pasted into new run")),
            Err::<(), _>(CANCELLED.into())
        );
        assert_eq!(
            with_current_run(&active, &fresh, || Ok("new result")),
            Ok("new result")
        );
    }

    #[tokio::test]
    async fn cancellation_aborts_waiting_for_http_response() {
        use super::*;
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/api/v1/recognitions",
            listener.local_addr().unwrap()
        );
        let (received_tx, received_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut buffer = [0; 8192];
            assert!(socket.read(&mut buffer).unwrap() > 0);
            let _ = received_tx.send(());
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });
        let path =
            std::env::temp_dir().join(format!("pill-cancel-test-{}.wav", std::process::id()));
        tokio::fs::write(&path, b"test audio").await.unwrap();
        let run = Arc::new(TranscriptionRun::new(1));
        let request_run = run.clone();
        let request_path = path.clone();
        let task = tokio::spawn(async move {
            recognize_for_run(&request_run, url, request_path.to_str().unwrap()).await
        });
        tokio::time::timeout(Duration::from_secs(3), received_rx)
            .await
            .unwrap()
            .unwrap();
        run.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.err(), Some(CANCELLED.into()));
        let _ = release_tx.send(());
        server.join().unwrap();
        tokio::fs::remove_file(path).await.unwrap();
    }
}

#[tauri::command]
async fn stop_recording(app: AppHandle) -> Result<RecordingFile, String> {
    let session = app
        .state::<AppState>()
        .recorder
        .lock()
        .map_err(|e| e.to_string())?
        .session
        .take()
        .ok_or("当前没有录音")?;
    let run = session.run.clone();
    run.check()?;
    app.emit("recording-processing", ())
        .map_err(|e| e.to_string())?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        session.stop.send(()).map_err(|e| e.to_string())?;
        session
            .done
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?
    })
    .await
    .map_err(|e| e.to_string())?;
    match result {
        Ok(path) if run.check().is_ok() => Ok(RecordingFile {
            path: path.to_string_lossy().to_string(),
            run_id: run.id,
        }),
        other => {
            if let Ok(path) = &other {
                let _ = std::fs::remove_file(path);
            }
            finish_run(&app, run.id).await;
            Err(other.err().unwrap_or_else(|| CANCELLED.into()))
        }
    }
}

async fn recognize(url: String, path: &str) -> Result<Recognition, String> {
    let data = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
    let part = reqwest::multipart::Part::bytes(data)
        .file_name("recording.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .part("audio", part)
        .text("language", "auto");
    reqwest::Client::new()
        .post(url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

async fn recognize_for_run(
    run: &TranscriptionRun,
    url: String,
    path: &str,
) -> Result<Recognition, String> {
    run.check()?;
    tokio::select! {
        biased;
        _ = run.cancellation() => Err(CANCELLED.into()),
        result = recognize(url, path) => result,
    }
}

#[tauri::command]
async fn transcribe_file(app: AppHandle, file: RecordingFile) -> Result<String, String> {
    if PathBuf::from(&file.path) != recording_path(file.run_id) {
        return Err("无效的录音文件".into());
    }
    let run = app
        .state::<AppState>()
        .active
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .filter(|run| run.id == file.run_id)
        .cloned()
        .ok_or(CANCELLED)?;
    let result = async {
        run.check()?;
        let base = app
            .state::<AppState>()
            .server_url
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        let url = recognition_url(&base)?;
        let result = recognize_for_run(&run, url, &file.path).await?;
        run.check()?;
        let text = result.raw_text.clone();
        let paste_app = app.clone();
        let paste_run = run.clone();
        let (paste_tx, paste_rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let outcome = (|| -> Result<(), String> {
                // Cancellation and the final paste are serialized with the active
                // run, so a delayed reply can never paste into a newer session.
                let state = paste_app.state::<AppState>();
                with_current_run(&state.active, &paste_run, || {
                    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
                    clipboard.set_text(text).map_err(|e| e.to_string())?;
                    let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
                        .map_err(|e| e.to_string())?;
                    use enigo::{Direction, Key, Keyboard};
                    let modifier = if cfg!(target_os = "macos") {
                        Key::Meta
                    } else {
                        Key::Control
                    };
                    enigo
                        .key(modifier, Direction::Press)
                        .map_err(|e| e.to_string())?;
                    let pasted = enigo
                        .key(Key::Unicode('v'), Direction::Click)
                        .map_err(|e| e.to_string());
                    let released = enigo
                        .key(modifier, Direction::Release)
                        .map_err(|e| e.to_string());
                    pasted?;
                    released?;
                    Ok(())
                })
            })();
            let _ = paste_tx.send(outcome);
        })
        .map_err(|e| e.to_string())?;
        paste_rx.await.map_err(|e| e.to_string())??;
        Ok(result.raw_text)
    }
    .await;
    let _ = tokio::fs::remove_file(&file.path).await;
    finish_run(&app, run.id).await;
    result
}

fn normalize_server_url(url: &str) -> Result<String, String> {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() {
        return Ok(String::new());
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| "请输入有效的 HTTP 或 HTTPS 后端地址")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("请输入有效的 HTTP 或 HTTPS 后端地址".into());
    }
    Ok(url.into())
}

fn recognition_url(base: &str) -> Result<String, String> {
    let base = normalize_server_url(base)?;
    if base.is_empty() {
        return Err("后端地址未设置".into());
    }
    // The configured base already includes the API prefix.
    Ok(format!("{base}/recognitions"))
}

async fn check_backend(base: &str) -> Result<(), String> {
    let base = normalize_server_url(base)?;
    if base.is_empty() {
        return Err("后端地址未设置".into());
    }
    let response = reqwest::Client::new()
        .get(format!("{base}/health"))
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .map_err(|_| "无法连接后端，请检查地址和服务状态")?;
    if !response.status().is_success() {
        return Err(format!(
            "后端连接失败（HTTP {}）",
            response.status().as_u16()
        ));
    }
    let body: serde_json::Value = response.json().await.map_err(|_| "后端健康检查响应无效")?;
    if body.get("status").and_then(|status| status.as_str()) != Some("ok") {
        return Err("后端健康检查未通过".into());
    }
    Ok(())
}

#[tauri::command]
async fn check_server_connection(state: State<'_, AppState>) -> Result<(), String> {
    let base = state.server_url.lock().map_err(|e| e.to_string())?.clone();
    check_backend(&base).await
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    #[tokio::test]
    async fn readiness_requires_a_successful_health_response() {
        use std::io::{Read, Write};
        assert_eq!(check_backend("").await, Err("后端地址未设置".into()));
        for (status, body, healthy) in [
            ("200 OK", r#"{"status":"ok","version":"v0.1.0"}"#, true),
            ("200 OK", r#"{"status":"error"}"#, false),
            ("200 OK", "not-json", false),
            ("503 Unavailable", r#"{"status":"ok"}"#, false),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}/api/v1/", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                }
                assert!(request.starts_with(b"GET /api/v1/health HTTP/1.1\r\n"));
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            assert_eq!(check_backend(&base).await.is_ok(), healthy);
            server.join().unwrap();
        }
    }

    #[test]
    fn validates_api_bases_and_preserves_proxy_paths() {
        assert_eq!(normalize_server_url("  ").unwrap(), "");
        assert!(recognition_url("").is_err());
        assert_eq!(
            recognition_url("http://localhost:8080/api/v1/").unwrap(),
            "http://localhost:8080/api/v1/recognitions"
        );
        assert_eq!(
            recognition_url("https://example.com/api/v1/").unwrap(),
            "https://example.com/api/v1/recognitions"
        );
        assert_eq!(
            recognition_url("https://example.com/proxy/asr/").unwrap(),
            "https://example.com/proxy/asr/recognitions"
        );
        for invalid in [
            "example.com",
            "file:///tmp/audio",
            "https://example.com/?token=x",
            "https://user:password@example.com",
        ] {
            assert!(normalize_server_url(invalid).is_err());
        }
    }
}

#[tauri::command]
fn set_server_url(app: AppHandle, state: State<'_, AppState>, url: String) -> Result<(), String> {
    let url = normalize_server_url(&url)?;
    // Both setters take these locks in this order so concurrent updates cannot
    // replace the other setting with an older value in the persisted file.
    let shortcut = state.shortcut.lock().map_err(|e| e.to_string())?;
    let mut server_url = state.server_url.lock().map_err(|e| e.to_string())?;
    persist_settings(&app, &shortcut, &url)?;
    *server_url = url;
    *state.settings_warning.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    let shortcut = state.shortcut.lock().map_err(|e| e.to_string())?.clone();
    let settings_warning = state
        .settings_warning
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let shortcut_warning = None;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let shortcut_warning = if is_modifier(&shortcut) {
        modifier_shortcut::warning()
    } else {
        shortcut_warning
    };
    Ok(Settings {
        settings_warning,
        developer_options: developer_options_enabled(),
        shortcut_warning,
        shortcut,
        server_url: state.server_url.lock().map_err(|e| e.to_string())?.clone(),
    })
}

// Release the registered shortcut so the webview can receive the same keys
// while recording a replacement. Restore it on completion or loss of focus.
#[tauri::command]
fn set_shortcut_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    capturing: bool,
) -> Result<(), String> {
    let current = state.shortcut.lock().map_err(|e| e.to_string())?;
    let mut active = state.shortcut_capturing.lock().map_err(|e| e.to_string())?;
    if *active == capturing {
        return Ok(());
    }
    if capturing {
        unbind_named_shortcut(&app, &current)?;
    } else {
        bind_named_shortcut(&app, &current)?;
    }
    *active = capturing;
    Ok(())
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
    if !is_modifier(&shortcut) {
        parse_shortcut(&shortcut)?;
    }
    let mut current = state.shortcut.lock().map_err(|e| e.to_string())?;
    if *state.shortcut_capturing.lock().map_err(|e| e.to_string())? {
        return Err("请先完成快捷键录入".into());
    }
    let server_url = state.server_url.lock().map_err(|e| e.to_string())?;
    if *current == shortcut {
        persist_settings(&app, &shortcut, &server_url)?;
        *state.settings_warning.lock().map_err(|e| e.to_string())? = None;
        return Ok(());
    }
    unbind_named_shortcut(&app, &current)?;
    if let Err(error) = bind_named_shortcut(&app, &shortcut) {
        let _ = bind_named_shortcut(&app, &current);
        return Err(error);
    }
    if let Err(error) = persist_settings(&app, &shortcut, &server_url) {
        let _ = unbind_named_shortcut(&app, &shortcut);
        if let Err(restore_error) = bind_named_shortcut(&app, &current) {
            return Err(format!("{error}；恢复原快捷键失败: {restore_error}"));
        }
        return Err(error);
    }
    *current = shortcut;
    *state.settings_warning.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

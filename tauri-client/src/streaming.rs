use crate::{
    authorize, backend_client, ensure_backend_success, recognition_url, Recognition,
    TranscriptionRun, CANCELLED,
};
use serde::Deserialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

// A slow connection must never block the capture thread or drop audio silently.
// On overflow abort this stream. The complete local WAV is the fallback when file ASR is enabled.
pub(super) struct Upload {
    sender: mpsc::Sender<Vec<u8>>,
    abort: tokio::task::AbortHandle,
}

impl Upload {
    pub(super) fn send(&mut self, samples: &[i16]) {
        if samples.is_empty() || self.sender.is_closed() {
            return;
        }
        let pcm = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        if self.sender.try_send(pcm).is_err() {
            self.abort.abort();
        }
    }
}

pub(super) fn start(run: Arc<TranscriptionRun>, sample_rate: u32, channels: u16) -> Upload {
    let (sender, receiver) = mpsc::channel(8);
    let (mut result_tx, result_rx) = oneshot::channel();
    *run.streaming_result.lock().unwrap() = Some(result_rx);
    let task = tauri::async_runtime::spawn(async move {
        let result = tokio::select! {
            biased;
            _ = run.cancellation() => Err(CANCELLED.into()),
            _ = result_tx.closed() => return,
            result = tokio::time::timeout(Duration::from_secs(300), recognize(&run, receiver, sample_rate, channels)) => {
                result.unwrap_or_else(|_| Err("流式识别超时".into()))
            }
        };
        let _ = result_tx.send(result);
    });
    Upload {
        sender,
        abort: task.inner().abort_handle(),
    }
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
    result: Option<Recognition>,
}

async fn recognize(
    run: &TranscriptionRun,
    receiver: mpsc::Receiver<Vec<u8>>,
    sample_rate: u32,
    channels: u16,
) -> Result<Recognition, String> {
    let mut metadata = serde_json::to_vec(&serde_json::json!({
        "sample_rate": sample_rate, "channels": channels,
        "language": "auto", "hotwords": run.hotwords,
    }))
    .map_err(|e| e.to_string())?;
    metadata.push(b'\n');
    let finished = Arc::new(AtomicBool::new(false));
    let uploaded = finished.clone();
    let partials = run.live_partials.lock().map_err(|e| e.to_string())?.clone();
    let mut transcript = String::new();
    let body = futures_util::stream::unfold(
        (Some(metadata), receiver),
        move |(metadata, mut receiver)| {
            let uploaded = uploaded.clone();
            async move {
                let chunk = match metadata {
                    Some(metadata) => metadata,
                    None => match receiver.recv().await {
                        Some(pcm) => pcm,
                        None => {
                            uploaded.store(true, Ordering::SeqCst);
                            return None;
                        }
                    },
                };
                Some((Ok::<_, std::io::Error>(chunk), (None, receiver)))
            }
        },
    );
    let request = backend_client()?
        .post(format!(
            "{}/stream",
            recognition_url(&run.backend.server_url)?
        ))
        .header("Content-Type", "application/octet-stream")
        .body(reqwest::Body::wrap_stream(body));
    let mut response = authorize(request, &run.backend.api_key)
        .send()
        .await
        .map_err(|_| "无法连接流式识别接口")?;
    ensure_backend_success(&response)?;
    if response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| !v.starts_with("application/x-ndjson"))
    {
        return Err("流式识别响应格式无效".into());
    }
    let mut pending = Vec::new();
    let mut total = 0;
    while let Some(chunk) = response.chunk().await.map_err(|_| "流式识别连接中断")? {
        total += chunk.len();
        if total > 2 << 20 {
            return Err("流式识别响应过大".into());
        }
        pending.extend_from_slice(&chunk);
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let event: Event =
                serde_json::from_slice(&pending[..end]).map_err(|_| "流式识别响应无效")?;
            pending.drain(..=end);
            match event.kind.as_str() {
                "ready" => {}
                "partial" if run.check().is_ok() && !event.text.is_empty() => {
                    // partial.text is a delta. The pill shows the accumulated transcript.
                    transcript.push_str(&event.text);
                    if let Some(partials) = &partials {
                        let _ = partials.send(transcript.clone());
                    }
                }
                "partial" => {}
                "final" if finished.load(Ordering::SeqCst) => {
                    return event.result.ok_or_else(|| "流式识别缺少最终结果".into())
                }
                _ => return Err("流式识别未正常完成".into()),
            }
        }
        if pending.len() > 256 << 10 {
            return Err("流式识别响应过大".into());
        }
    }
    Err("流式识别缺少最终结果".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};

    fn headers(socket: &mut BufReader<TcpStream>) -> String {
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            assert!(socket.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                return headers;
            }
            headers.push_str(&line);
        }
    }

    fn chunk(socket: &mut BufReader<TcpStream>) -> Vec<u8> {
        let mut line = String::new();
        socket.read_line(&mut line).unwrap();
        let size = usize::from_str_radix(line.trim(), 16).unwrap();
        let mut bytes = vec![0; size];
        socket.read_exact(&mut bytes).unwrap();
        let mut end = [0; 2];
        socket.read_exact(&mut end).unwrap();
        assert_eq!(&end, b"\r\n");
        bytes
    }

    fn emit(socket: &mut BufReader<TcpStream>, bytes: &[u8]) {
        write!(socket.get_mut(), "{:x}\r\n", bytes.len()).unwrap();
        socket.get_mut().write_all(bytes).unwrap();
        socket.get_mut().write_all(b"\r\n").unwrap();
    }

    fn accept(listener: &TcpListener) -> BufReader<TcpStream> {
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        BufReader::new(socket)
    }

    #[tokio::test]
    async fn recording_to_history_scenarios_preserve_audio_and_count_once() {
        use crate::{cleanup_recording, history, recording, save_recording, with_current_run};
        use std::sync::Mutex;
        let directory = tempfile::tempdir().unwrap();
        let mut store = history::Store::open(&directory.path().join("history")).unwrap();
        let mut count = 0;
        for mode in [
            "stream",
            "disconnect-during-capture",
            "disconnect-after-eof",
            "legacy",
        ] {
            for disk_failure in [false, true] {
                count += 1;
                let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                let mut run = TranscriptionRun::new(count);
                run.backend.server_url =
                    format!("http://{}/api/v1", listener.local_addr().unwrap());
                run.backend.api_key = "snapshot-key".into();
                run.hotwords = "OAuth\n语音".into();
                let run = Arc::new(run);
                let mut source = vec![0; 3200];
                source.extend(vec![1000; 6400]);
                source.extend(vec![300; 3200]);
                let mut expected =
                    recording::Audio::new(16000, 1, recording::MAX_AUDIO_BYTES).unwrap();
                expected.push(source.iter().copied());
                let wav = expected.finish().unwrap();
                let expected_pcm = hound::WavReader::new(std::io::Cursor::new(&wav))
                    .unwrap()
                    .samples::<i16>()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                let expected_wav = wav.clone();
                let (seen_tx, seen_rx) = oneshot::channel();
                let server = std::thread::spawn(move || {
                    if mode != "legacy" {
                        let mut socket = accept(&listener);
                        let request = headers(&mut socket);
                        assert!(request
                            .to_ascii_lowercase()
                            .contains("authorization: bearer snapshot-key"));
                        let metadata: serde_json::Value =
                            serde_json::from_slice(&chunk(&mut socket)).unwrap();
                        assert_eq!(metadata["hotwords"], "OAuth\n语音");
                        let mut pcm = chunk(&mut socket);
                        seen_tx.send(()).unwrap();
                        if mode != "disconnect-during-capture" {
                            loop {
                                let data = chunk(&mut socket);
                                if data.is_empty() {
                                    break;
                                }
                                pcm.extend(data);
                            }
                            let samples: Vec<_> = pcm
                                .chunks_exact(2)
                                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                                .collect();
                            assert_eq!(samples, expected_pcm);
                            if mode == "stream" {
                                let body = "{\"type\":\"final\",\"result\":{\"raw_text\":\"完整录音\",\"polished_text\":\"完整录音。\"}}\n";
                                write!(socket.get_mut(), "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                                return;
                            }
                        }
                        drop(socket);
                    }
                    let (mut socket, _) = listener.accept().unwrap();
                    let request = crate::read_test_request(&mut socket);
                    assert!(request.starts_with(b"POST /api/v1/recognitions "));
                    assert!(request
                        .windows(expected_wav.len())
                        .any(|bytes| bytes == expected_wav));
                    assert!(String::from_utf8_lossy(&request).contains("OAuth\n语音"));
                    let body = r#"{"raw_text":"完整录音","polished_text":"完整录音。"}"#;
                    write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                });
                let mut audio =
                    recording::Audio::new(16000, 1, recording::MAX_AUDIO_BYTES).unwrap();
                let mut upload = None;
                audio.push(source[..6400].iter().copied());
                let initial = audio.pending(false);
                if mode != "legacy" {
                    let mut started = start(run.clone(), 16000, 1);
                    started.send(&initial);
                    upload = Some(started);
                    tokio::time::timeout(Duration::from_secs(3), seen_rx)
                        .await
                        .unwrap()
                        .unwrap();
                }
                audio.push(source[6400..].iter().copied());
                if let Some(upload) = upload.as_mut() {
                    upload.send(&audio.pending(true));
                }
                drop(upload);
                let data = audio.finish().unwrap();
                assert_eq!(data, wav);
                let path = if disk_failure {
                    directory.path().join(format!("missing/{count}.wav"))
                } else {
                    directory.path().join(format!("{count}.wav"))
                };
                save_recording(&run, &path, data).unwrap();
                assert_eq!(path.exists(), !disk_failure);
                assert_eq!(run.warnings.lock().unwrap().is_empty(), !disk_failure);
                run.begin_transcription().unwrap();
                let result = crate::recognize_for_run(
                    &run,
                    recognition_url(&run.backend.server_url).unwrap(),
                    path.to_str().unwrap(),
                )
                .await
                .unwrap();
                assert_eq!(result.output_text(), "完整录音。");
                let active = Mutex::new(Some(run.clone()));
                with_current_run(&active, &run, || {
                    run.finalizing.store(true, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap();
                if let Some(data) = run.recording_bytes.lock().unwrap().take() {
                    store
                        .save_bytes(
                            &run.stamp,
                            &data,
                            &result.raw_text,
                            result.polished_text.as_deref(),
                        )
                        .unwrap();
                } else {
                    store
                        .save(
                            &run.stamp,
                            &path,
                            &result.raw_text,
                            result.polished_text.as_deref(),
                        )
                        .unwrap();
                }
                assert!(run.begin_transcription().is_err());
                assert!(!run.cancel());
                let archived = store.recording(&run.stamp.id).unwrap();
                assert_eq!(std::fs::read(&archived).unwrap(), wav);
                assert_eq!(
                    store
                        .entry(&run.stamp.id)
                        .unwrap()
                        .unwrap()
                        .audio_duration_ms,
                    700
                );
                assert_eq!(
                    store
                        .insights()
                        .unwrap()
                        .days
                        .iter()
                        .map(|day| day.recognition_count)
                        .sum::<i64>(),
                    count as i64
                );
                cleanup_recording(&path, false).unwrap();
                assert!(!path.exists());
                store.delete(&run.stamp.id).unwrap();
                assert!(!archived.exists());
                assert_eq!(
                    store
                        .insights()
                        .unwrap()
                        .days
                        .iter()
                        .map(|day| day.recognition_count)
                        .sum::<i64>(),
                    count as i64
                );
                server.join().unwrap();
            }
        }
        drop(store);
        let restored = history::Store::open(&directory.path().join("history")).unwrap();
        assert!(restored.page(None).unwrap().entries.is_empty());
        assert_eq!(
            restored.insights().unwrap().character_count,
            4 * count as i64
        );
    }

    #[tokio::test]
    async fn cancellation_after_stop_removes_audio_and_never_accepts_a_late_result() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cancelled.wav");
        let run = Arc::new(TranscriptionRun::new(11));
        let mut audio = crate::recording::Audio::new(16000, 1, 4044).unwrap();
        audio.push(std::iter::repeat(1000));
        crate::save_recording(&run, &path, audio.finish().unwrap()).unwrap();
        let (tx, rx) = oneshot::channel();
        *run.streaming_result.lock().unwrap() = Some(rx);
        let waiting = run.clone();
        let task_path = path.clone();
        let task = tokio::spawn(async move {
            crate::recognize_for_run(
                &waiting,
                "http://unused.invalid".into(),
                task_path.to_str().unwrap(),
            )
            .await
        });
        tokio::task::yield_now().await;
        run.cancel();
        let _ = tx.send(Ok(Recognition {
            raw_text: "late".into(),
            polished_text: None,
        }));
        assert_eq!(task.await.unwrap().err(), Some(CANCELLED.into()));
        crate::cleanup_recording(&path, false).unwrap();
        assert!(!path.exists());
        let mut store = crate::history::Store::open(&directory.path().join("history")).unwrap();
        let active = std::sync::Mutex::new(Some(run.clone()));
        assert!(crate::with_current_run(&active, &run, || store
            .save(&run.stamp, &path, "late", None))
        .is_err());
        assert!(store.page(None).unwrap().entries.is_empty());
        assert!(store.insights().unwrap().days.is_empty());
    }

    #[tokio::test]
    async fn slow_upload_aborts_without_blocking_or_losing_the_recording() {
        let (sender, _receiver) = mpsc::channel(1);
        let (finished_tx, finished_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _keep_alive = finished_tx;
            std::future::pending::<()>().await;
        });
        let mut upload = Upload {
            sender,
            abort: task.abort_handle(),
        };
        let mut audio = crate::recording::Audio::new(16000, 1, 32044).unwrap();
        for _ in 0..10 {
            audio.push(std::iter::repeat_n(1000, 1600));
            upload.send(&audio.pending(false));
        }
        assert!(tokio::time::timeout(Duration::from_secs(1), finished_rx)
            .await
            .unwrap()
            .is_err());
        let data = audio.finish().unwrap();
        assert_eq!(
            hound::WavReader::new(std::io::Cursor::new(data))
                .unwrap()
                .duration(),
            16000
        );
    }

    #[tokio::test]
    async fn streams_pcm_before_stop_and_accepts_only_the_final_result() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut run = TranscriptionRun::new(1);
        run.backend.server_url = format!("http://{}/api/v1", listener.local_addr().unwrap());
        run.backend.api_key = "stream-key".into();
        run.hotwords = "OAuth\n语音".into();
        let run = Arc::new(run);
        let (live_tx, live_rx) = std::sync::mpsc::channel();
        *run.live_partials.lock().unwrap() = Some(live_tx);
        let (partial_tx, partial_rx) = oneshot::channel();
        let server = std::thread::spawn(move || {
            let mut socket = accept(&listener);
            let request = headers(&mut socket);
            assert!(request.starts_with("POST /api/v1/recognitions/stream "));
            assert!(request
                .to_ascii_lowercase()
                .contains("authorization: bearer stream-key\r\n"));
            let metadata = chunk(&mut socket);
            let metadata: serde_json::Value = serde_json::from_slice(&metadata).unwrap();
            assert_eq!(metadata["hotwords"], "OAuth\n语音");
            assert_eq!(metadata["sample_rate"], 48000);
            assert_eq!(metadata["channels"], 2);
            assert_eq!(chunk(&mut socket), vec![1, 0, 255, 255]);
            socket.get_mut().write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
            emit(
                &mut socket,
                "{\"type\":\"ready\"}\n{\"type\":\"partial\",\"text\":\"半\"}\n{\"type\":\"partial\",\"text\":\"句\"}\n".as_bytes(),
            );
            partial_tx.send(()).unwrap();
            assert_eq!(chunk(&mut socket), vec![2, 0, 254, 255]);
            assert!(chunk(&mut socket).is_empty());
            // Deliberately split JSON and a UTF-8 character across HTTP chunks.
            let result = "{\"type\":\"final\",\"result\":{\"raw_text\":\"语音\",\"polished_text\":\"语音。\"}}\n".as_bytes();
            for byte in result {
                emit(&mut socket, &[*byte]);
            }
            socket.get_mut().write_all(b"0\r\n\r\n").unwrap();
        });
        let mut upload = start(run.clone(), 48000, 2);
        upload.send(&[1, -1]);
        tokio::time::timeout(Duration::from_secs(3), partial_rx)
            .await
            .unwrap()
            .unwrap();
        let mut seen = String::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while seen != "半句" && std::time::Instant::now() < deadline {
            match live_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(text) => seen = text,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(error) => panic!("{error}"),
            }
        }
        assert_eq!(seen, "半句");
        upload.send(&[2, -2]);
        drop(upload);
        let result =
            crate::recognize_for_run(&run, "http://unused.invalid".into(), "/no-file-needed")
                .await
                .unwrap();
        assert_eq!(result.raw_text, "语音");
        assert_eq!(result.output_text(), "语音。");
        server.join().unwrap();
    }

    #[tokio::test]
    async fn unavailable_or_broken_stream_retries_the_complete_wav() {
        for response in [
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 501 Not Implemented\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://unused.invalid\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n{\"type\":\"partial\",\"text\":\"incomplete\"}\n",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut run = TranscriptionRun::new(2);
            run.backend.server_url = format!("http://{}/api/v1", listener.local_addr().unwrap());
            run.backend.api_key = "fallback-key".into();
            run.hotwords = "OAuth".into();
            let run = Arc::new(run);
            let server = std::thread::spawn(move || {
                let mut socket = accept(&listener);
                headers(&mut socket);
                while !chunk(&mut socket).is_empty() {}
                socket.get_mut().write_all(response.as_bytes()).unwrap();
                drop(socket);
                let (mut socket, _) = listener.accept().unwrap();
                let request = crate::read_test_request(&mut socket);
                let request = String::from_utf8_lossy(&request);
                assert!(request.starts_with("POST /api/v1/recognitions "));
                assert!(request.to_ascii_lowercase().contains("authorization: bearer fallback-key"));
                assert!(request.contains("complete WAV"));
                assert!(request.contains("OAuth"));
                let body = r#"{"raw_text":"complete result"}"#;
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), "complete WAV").unwrap();
            let mut upload = start(run.clone(), 16000, 1);
            upload.send(&[1, 2]);
            drop(upload);
            let result = crate::recognize_for_run(&run, recognition_url(&run.backend.server_url).unwrap(), file.path().to_str().unwrap()).await.unwrap();
            assert_eq!(result.raw_text, "complete result");
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn rejects_final_result_while_capture_is_still_open() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut run = TranscriptionRun::new(4);
        run.backend.server_url = format!("http://{}/api/v1", listener.local_addr().unwrap());
        let run = Arc::new(run);
        let server = std::thread::spawn(move || {
            let mut socket = accept(&listener);
            headers(&mut socket);
            chunk(&mut socket);
            let body = "{\"type\":\"final\",\"result\":{\"raw_text\":\"too early\"}}\n";
            write!(socket.get_mut(), "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let _upload = start(run.clone(), 16000, 1);
        let result = run.streaming_result.lock().unwrap().take().unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(3), result)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn cancellation_closes_stream_without_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut run = TranscriptionRun::new(3);
        run.backend.server_url = format!("http://{}/api/v1", listener.local_addr().unwrap());
        let run = Arc::new(run);
        let (started_tx, started_rx) = oneshot::channel();
        let (closed_tx, closed_rx) = oneshot::channel();
        let server = std::thread::spawn(move || {
            let mut socket = accept(&listener);
            headers(&mut socket);
            chunk(&mut socket);
            started_tx.send(()).unwrap();
            let mut bytes = Vec::new();
            let result = socket.read_to_end(&mut bytes);
            assert!(
                result.is_ok() || result.unwrap_err().kind() == std::io::ErrorKind::ConnectionReset
            );
            closed_tx.send(()).unwrap();
        });
        let _upload = start(run.clone(), 16000, 1);
        started_rx.await.unwrap();
        let waiting = run.clone();
        let recognition = tokio::spawn(async move {
            crate::recognize_for_run(&waiting, "http://unused.invalid".into(), "/no-file-needed")
                .await
        });
        run.cancel();
        let result = tokio::time::timeout(Duration::from_secs(1), recognition)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.err(), Some(CANCELLED.into()));
        tokio::time::timeout(Duration::from_secs(1), closed_rx)
            .await
            .unwrap()
            .unwrap();
        server.join().unwrap();
    }

    #[tokio::test]
    async fn health_only_enables_explicit_stream_capability() {
        for (capabilities, expected) in [
            ("", false),
            (r#", "capabilities":{"streaming_asr":false}"#, false),
            (r#", "capabilities":{"streaming_asr":true}"#, true),
            (r#", "capabilities":{"streaming_asr":true,"file_asr":false}"#, true),
            (r#", "capabilities":{"streaming_asr":false,"file_asr":true}"#, false),
            (r#", "limits":{"max_audio_bytes":2048}"#, false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/api/v1", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let mut socket = accept(&listener);
                headers(&mut socket);
                let body = format!(r#"{{"status":"ok"{capabilities}}}"#);
                write!(
                    socket.get_mut(),
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            });
            let capabilities_body = crate::backend_capabilities(&url, "").await.unwrap();
            assert_eq!(capabilities_body.streaming, expected);
            assert_eq!(capabilities_body.file, !capabilities.contains("\"file_asr\":false"));
            assert_eq!(
                capabilities_body.max_audio_bytes,
                if capabilities.contains("2048") {
                    2048
                } else {
                    crate::recording::MAX_AUDIO_BYTES as u64
                }
            );
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn health_rejects_a_backend_with_no_asr_interface() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/v1", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut socket = accept(&listener);
            headers(&mut socket);
            let body = r#"{"status":"ok","capabilities":{"streaming_asr":false,"file_asr":false}}"#;
            write!(
                socket.get_mut(),
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        match crate::backend_capabilities(&url, "").await {
            Err(error) => assert_eq!(error, "后端未启用语音识别"),
            Ok(_) => panic!("accepted a backend with no ASR interface"),
        }
        server.join().unwrap();
    }

    #[tokio::test]
    async fn streaming_only_does_not_retry_with_the_complete_wav() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut run = TranscriptionRun::new(8);
        run.backend.server_url = format!("http://{}/api/v1", listener.local_addr().unwrap());
        run.file_asr_enabled.store(false, Ordering::SeqCst);
        let run = Arc::new(run);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut socket = accept(&listener);
            headers(&mut socket);
            while !chunk(&mut socket).is_empty() {}
            socket.get_mut().write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n{\"type\":\"partial\",\"text\":\"incomplete\"}\n").unwrap();
            drop(socket);
            let _ = done_rx.recv_timeout(Duration::from_secs(3));
            listener.set_nonblocking(true).unwrap();
            assert!(listener.accept().is_err());
        });
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "complete WAV").unwrap();
        let mut upload = start(run.clone(), 16000, 1);
        upload.send(&[1, 2]);
        drop(upload);
        let error = match crate::recognize_for_run(
            &run,
            recognition_url(&run.backend.server_url).unwrap(),
            file.path().to_str().unwrap(),
        )
        .await
        {
            Err(error) => error,
            Ok(_) => panic!("streaming-only retried or accepted an incomplete stream"),
        };
        assert_ne!(error, CANCELLED);
        assert!(!error.is_empty());
        done_tx.send(()).unwrap();
        server.join().unwrap();
    }

    #[tokio::test]
    #[ignore = "Requires OPEN_TYPELESS_TEST_BASE_URL, OPEN_TYPELESS_TEST_API_KEY and OPEN_TYPELESS_TEST_WAV"]
    async fn live_backend_smoke() {
        let mut run = TranscriptionRun::new(100);
        run.backend.server_url = std::env::var("OPEN_TYPELESS_TEST_BASE_URL").unwrap();
        run.backend.api_key = std::env::var("OPEN_TYPELESS_TEST_API_KEY").unwrap();
        assert!(
            crate::backend_capabilities(&run.backend.server_url, &run.backend.api_key)
                .await
                .unwrap()
                .streaming
        );
        let mut audio =
            hound::WavReader::open(std::env::var("OPEN_TYPELESS_TEST_WAV").unwrap()).unwrap();
        let spec = audio.spec();
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);
        let samples: Vec<i16> = audio.samples().collect::<Result<_, _>>().unwrap();
        let run = Arc::new(run);
        let mut capture = crate::recording::Audio::new(
            spec.sample_rate,
            spec.channels,
            crate::recording::MAX_AUDIO_BYTES,
        )
        .unwrap();
        let mut upload = None;
        for chunk in samples.chunks(spec.sample_rate as usize * spec.channels as usize / 10) {
            capture.push(chunk.iter().copied());
            let pcm = capture.pending(false);
            if !pcm.is_empty() {
                upload
                    .get_or_insert_with(|| start(run.clone(), spec.sample_rate, spec.channels))
                    .send(&pcm);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let tail = capture.pending(true);
        if !tail.is_empty() {
            upload
                .get_or_insert_with(|| start(run.clone(), spec.sample_rate, spec.channels))
                .send(&tail);
        }
        let wav = capture.finish().unwrap();
        drop(upload);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recording.wav");
        crate::save_recording(&run, &path, wav.clone()).unwrap();
        // Inspect the streaming result directly so offline fallback cannot mask failure.
        let result = run.streaming_result.lock().unwrap().take().unwrap();
        let result = tokio::time::timeout(Duration::from_secs(60), result)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!result.raw_text.trim().is_empty());
        if let Ok(expected) = std::env::var("OPEN_TYPELESS_TEST_EXPECTED") {
            assert_eq!(result.raw_text, expected);
        }
        let mut store = crate::history::Store::open(&directory.path().join("history")).unwrap();
        store
            .save(
                &run.stamp,
                &path,
                &result.raw_text,
                result.polished_text.as_deref(),
            )
            .unwrap();
        assert_eq!(
            std::fs::read(store.recording(&run.stamp.id).unwrap()).unwrap(),
            wav
        );
        crate::cleanup_recording(&path, false).unwrap();
        assert!(!path.exists());
        assert_eq!(
            store
                .insights()
                .unwrap()
                .days
                .iter()
                .map(|day| day.recognition_count)
                .sum::<i64>(),
            1
        );
    }
}

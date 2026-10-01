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
// On overflow abort this stream; the complete local WAV remains the fallback.
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
                "ready" | "partial" => (),
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
    async fn streams_pcm_before_stop_and_accepts_only_the_final_result() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut run = TranscriptionRun::new(1);
        run.backend.server_url = format!("http://{}/api/v1", listener.local_addr().unwrap());
        run.backend.api_key = "stream-key".into();
        run.hotwords = "OAuth\n语音".into();
        let run = Arc::new(run);
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
                b"{\"type\":\"ready\"}\n{\"type\":\"partial\",\"text\":\"half\"}\n",
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
            assert_eq!(
                crate::backend_capabilities(&url, "").await.unwrap(),
                expected
            );
            server.join().unwrap();
        }
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
        );
        let mut audio =
            hound::WavReader::open(std::env::var("OPEN_TYPELESS_TEST_WAV").unwrap()).unwrap();
        let spec = audio.spec();
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(spec.sample_format, hound::SampleFormat::Int);
        let samples: Vec<i16> = audio.samples().collect::<Result<_, _>>().unwrap();
        let run = Arc::new(run);
        let mut upload = start(run.clone(), spec.sample_rate, spec.channels);
        for chunk in samples.chunks(spec.sample_rate as usize * spec.channels as usize / 10) {
            upload.send(chunk);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        drop(upload);
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
    }
}

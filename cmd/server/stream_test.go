package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestStreamingAudioArrivesBeforeUploadEnds(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/audio/transcriptions/live" || r.URL.Query().Get("model") != "live-model" ||
			r.URL.Query().Get("prompt") != "OAuth\n语音" || r.URL.Query().Has("language") ||
			r.URL.Query().Get("sample_rate") != "16000" || r.URL.Query().Get("channels") != "1" ||
			r.URL.Query().Get("sample_format") != "s16le" || r.Header.Get("Authorization") != "" {
			t.Error("incorrect inference request")
		}
		if err := http.NewResponseController(w).EnableFullDuplex(); err != nil {
			t.Error(err)
			return
		}
		head := make([]byte, 4)
		if _, err := io.ReadFull(r.Body, head); err != nil || !bytes.Equal(head, []byte{1, 0, 2, 0}) {
			t.Error("missing initial PCM")
			return
		}
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"type\":\"transcript.text.delta\",\"delta\":\"语音\"}\n\n")
		w.(http.Flusher).Flush()
		tail, err := io.ReadAll(r.Body)
		if err != nil || !bytes.Equal(tail, []byte{3, 0, 4, 0}) {
			t.Error("missing final PCM")
			return
		}
		fmt.Fprint(w, "data: {\"type\":\"transcript.text.done\",\"text\":\"语音 OAuth\"}\n\ndata: [DONE]\n\n")
	}))
	defer upstream.Close()
	s := &server{inferenceURL: upstream.URL, inferenceStreamModel: "live-model", client: upstream.Client(), maxBytes: 1024, disableFileASR: true}
	polishCalls := 0
	llm := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request polishRequest
		if json.NewDecoder(r.Body).Decode(&request) != nil || len(request.Messages) == 0 ||
			!strings.Contains(request.Messages[len(request.Messages)-1].Content, "语音 OAuth") {
			t.Error("polishing did not receive the final raw transcript")
		}
		polishCalls++
		fmt.Fprint(w, `{"choices":[{"message":{"content":"Polished final."},"finish_reason":"stop"}]}`)
	}))
	defer llm.Close()
	s.polisher = &polisher{baseURL: llm.URL, client: llm.Client(), timeout: time.Second}
	backend := httptest.NewServer(s.handler("test-key"))
	defer backend.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	reader, writer := io.Pipe()
	defer reader.Close()
	defer writer.Close()
	request, _ := http.NewRequestWithContext(ctx, http.MethodPost, backend.URL+"/api/v1/recognitions/stream", reader)
	request.Header.Set("Authorization", "Bearer test-key")
	go func() {
		fmt.Fprintln(writer, `{"sample_rate":16000,"channels":1,"language":"auto","hotwords":"OAuth\n语音"}`)
		_, _ = writer.Write([]byte{1, 0, 2, 0})
	}()
	response, err := backend.Client().Do(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	decoder := json.NewDecoder(response.Body)
	for _, kind := range []string{"ready", "partial"} {
		var event streamEvent
		if err := decoder.Decode(&event); err != nil || event.Type != kind {
			t.Fatalf("expected %s before EOF: %+v %v", kind, event, err)
		}
	}
	_, _ = writer.Write([]byte{3, 0, 4, 0})
	_ = writer.Close()
	var final streamEvent
	if err := decoder.Decode(&final); err != nil || final.Type != "final" || final.Result == nil || final.Result.RawText != "语音 OAuth" || final.Result.PolishedText != "Polished final." {
		t.Fatalf("invalid final event: %+v %v", final, err)
	}
	llm.Close()
	if polishCalls != 1 {
		t.Fatalf("polished %d times", polishCalls)
	}
}

func TestStreamingRejectsInvalidAndIncompleteInput(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, r.Body)
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"type\":\"transcript.text.done\",\"text\":\"must not be accepted\"}\n\n")
	}))
	defer upstream.Close()
	s := &server{inferenceURL: upstream.URL, inferenceStreamModel: "live", client: upstream.Client(), maxBytes: 4}
	backend := httptest.NewServer(s.handler("key"))
	defer backend.Close()
	for _, body := range []string{
		`{"sample_rate":0,"channels":1}` + "\n12",
		`{"sample_rate":16000,"channels":0}` + "\n12",
		`{"sample_rate":16000,"channels":1,"hotwords":"` + strings.Repeat("词", 334) + `"}` + "\n12",
		strings.Repeat("x", 4096),
		`{"sample_rate":16000,"channels":1}` + "\n",
		`{"sample_rate":16000,"channels":2}` + "\n12",
		`{"sample_rate":16000,"channels":1}` + "\n123456",
	} {
		req, _ := http.NewRequest(http.MethodPost, backend.URL+"/api/v1/recognitions/stream", strings.NewReader(body))
		req.Header.Set("Authorization", "Bearer key")
		client := *backend.Client()
		client.Timeout = 3 * time.Second
		resp, err := client.Do(req)
		if err != nil {
			t.Fatal(err)
		}
		data, _ := io.ReadAll(resp.Body)
		resp.Body.Close()
		if strings.Contains(string(data), `"type":"final"`) || (resp.StatusCode == 200 && !strings.Contains(string(data), `"type":"error"`)) {
			t.Fatalf("accepted invalid PCM: %s", data)
		}
	}
}

func TestStreamCapabilityAndCancellation(t *testing.T) {
	for _, model := range []string{"", "stream-model"} {
		w := httptest.NewRecorder()
		(&server{inferenceStreamModel: model}).health(w, httptest.NewRequest("GET", "/", nil))
		var body struct {
			Capabilities struct {
				Streaming bool `json:"streaming_asr"`
				File      bool `json:"file_asr"`
			} `json:"capabilities"`
		}
		if json.Unmarshal(w.Body.Bytes(), &body) != nil || body.Capabilities.Streaming != (model != "") || !body.Capabilities.File {
			t.Fatal(w.Body.String())
		}
	}
	w := httptest.NewRecorder()
	(&server{inferenceStreamModel: "live", disableFileASR: true}).health(w, httptest.NewRequest("GET", "/", nil))
	if !strings.Contains(w.Body.String(), `"streaming_asr":true`) || !strings.Contains(w.Body.String(), `"file_asr":false`) {
		t.Fatal(w.Body.String())
	}
	started, stopped := make(chan struct{}), make(chan struct{})
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.NewResponseController(w).EnableFullDuplex()
		w.Header().Set("Content-Type", "text/event-stream")
		w.(http.Flusher).Flush()
		close(started)
		_, _ = io.Copy(io.Discard, r.Body)
		<-r.Context().Done()
		close(stopped)
	}))
	defer upstream.Close()
	s := &server{inferenceURL: upstream.URL, inferenceStreamModel: "live", client: upstream.Client(), maxBytes: 1024}
	backend := httptest.NewServer(s.handler("key"))
	defer backend.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	reader, writer := io.Pipe()
	defer writer.Close()
	request, _ := http.NewRequestWithContext(ctx, "POST", backend.URL+"/api/v1/recognitions/stream", reader)
	request.Header.Set("Authorization", "Bearer key")
	go func() { fmt.Fprintln(writer, `{"sample_rate":16000,"channels":1}`); writer.Write([]byte{1, 0}) }()
	response, err := backend.Client().Do(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	select {
	case <-started:
	case <-time.After(3 * time.Second):
		t.Fatal("upstream did not start")
	}
	cancel()
	select {
	case <-stopped:
	case <-time.After(3 * time.Second):
		t.Fatal("upstream was not cancelled")
	}
}

func TestTranscriptSSERequiresExplicitFinal(t *testing.T) {
	for _, body := range []string{
		"data: {\"type\":\"transcript.text.delta\",\"delta\":\"half\"}\n\n",
		"data: [DONE]\n\n", "data: invalid\n\n",
		"data: {\"type\":\"transcript.text.done\"}\n\n",
		"data: {\"type\":\"error\"}\n\n",
		"data: " + strings.Repeat("x", 256<<10) + "\n\n",
	} {
		if _, err := readTranscriptEvents(strings.NewReader(body), func(string) error { return nil }); err == nil {
			t.Fatal("accepted incomplete SSE")
		}
	}
	body := ": keepalive\r\ndata: {\"type\":\"transcript.text.done\",\r\ndata: \"text\":\"完整\"}\r\n\r\n"
	if text, err := readTranscriptEvents(strings.NewReader(body), func(string) error { return nil }); err != nil || text != "完整" {
		t.Fatalf("%q %v", text, err)
	}
}

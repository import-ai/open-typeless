package main

import (
	"bytes"
	"encoding/json"
	"io"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"
)

func TestFileASRConfiguration(t *testing.T) {
	t.Setenv("INFERENCE_FILE_ASR", "")
	enabled, err := fileASREnabled()
	if err != nil || !enabled {
		t.Fatalf("default file ASR: %v %v", enabled, err)
	}
	for _, value := range []string{"0", "false", "off", "NO"} {
		t.Setenv("INFERENCE_FILE_ASR", value)
		enabled, err = fileASREnabled()
		if err != nil || enabled {
			t.Fatalf("%s enabled file ASR: %v %v", value, enabled, err)
		}
	}
	t.Setenv("INFERENCE_FILE_ASR", "maybe")
	if _, err = fileASREnabled(); err == nil {
		t.Fatal("accepted invalid INFERENCE_FILE_ASR")
	}
	if err := validateInference("audiocpp", "live", false); err != nil {
		t.Fatal(err)
	}
	if err := validateInference("legacy", "", false); err == nil {
		t.Fatal("streaming-only mode accepted without a streaming model")
	}
	if err := validateInference("legacy", "live", true); err == nil {
		t.Fatal("streaming model accepted with the legacy protocol")
	}
	s := &server{disableFileASR: true, maxBytes: 32}
	response := httptest.NewRecorder()
	s.recognize(response, recognitionRequest(t, ""))
	if response.Code != http.StatusNotImplemented || !strings.Contains(response.Body.String(), "file ASR is not configured") {
		t.Fatalf("file endpoint stayed available: %d %s", response.Code, response.Body.String())
	}
}

func TestHealthPublishesConfiguredAudioLimit(t *testing.T) {
	s := &server{maxBytes: 123456, inferenceStreamModel: "live"}
	w := httptest.NewRecorder()
	s.health(w, httptest.NewRequest(http.MethodGet, "/api/v1/health", nil))
	var body struct {
		Limits struct {
			MaxBytes int64 `json:"max_audio_bytes"`
		} `json:"limits"`
	}
	if json.Unmarshal(w.Body.Bytes(), &body) != nil || body.Limits.MaxBytes != 123456 {
		t.Fatalf("incorrect recording limit: %s", w.Body.String())
	}
}

func TestRecognitionForwardsDictionaryContext(t *testing.T) {
	for _, hotwords := range []string{"", "OAuth\n语音\nego Lite\nAGENTS.md", strings.Repeat("a", 1000)} {
		t.Run(strconv.Itoa(len(hotwords))+"_bytes", func(t *testing.T) {
			var called atomic.Bool
			inference := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				called.Store(true)
				if r.URL.Path != "/transcribe" || r.URL.Query().Get("context") != hotwords || r.URL.Query().Get("language") != "auto" {
					t.Errorf("incorrect inference request: %s", r.URL)
				}
				audio, _ := io.ReadAll(r.Body)
				if !bytes.Equal(audio, []byte{0, 1, 128, 255}) {
					t.Errorf("audio changed: %v", audio)
				}
				w.Header().Set("Content-Type", "application/json")
				io.WriteString(w, `{"raw_text":"OAuth","language":"zh","audio_duration_ms":1000}`)
			}))
			defer inference.Close()
			s := &server{inferenceURL: inference.URL, client: inference.Client(), maxBytes: 12 << 20}
			response := httptest.NewRecorder()
			s.recognize(response, recognitionRequest(t, hotwords))
			if !called.Load() || response.Code != http.StatusOK || !strings.Contains(response.Body.String(), `"raw_text":"OAuth"`) || !strings.Contains(response.Body.String(), `"polished_text":"OAuth"`) {
				t.Fatalf("recognition failed: %d %s", response.Code, response.Body.String())
			}
		})
	}
}

func TestRecognitionRejectsOversizedUTF8Dictionary(t *testing.T) {
	s := &server{maxBytes: 12 << 20}
	response := httptest.NewRecorder()
	s.recognize(response, recognitionRequest(t, strings.Repeat("词", 334)))
	if response.Code != http.StatusBadRequest {
		t.Fatalf("expected 400, got %d", response.Code)
	}
}

func recognitionRequest(t *testing.T, hotwords string) *http.Request {
	t.Helper()
	var body bytes.Buffer
	form := multipart.NewWriter(&body)
	audio, err := form.CreateFormFile("audio", "recording.wav")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = audio.Write([]byte{0, 1, 128, 255}); err != nil {
		t.Fatal(err)
	}
	if err = form.WriteField("language", "auto"); err != nil {
		t.Fatal(err)
	}
	if err = form.WriteField("hotwords", hotwords); err != nil {
		t.Fatal(err)
	}
	if err = form.Close(); err != nil {
		t.Fatal(err)
	}
	r := httptest.NewRequest(http.MethodPost, "/api/v1/recognitions", &body)
	r.Header.Set("Content-Type", form.FormDataContentType())
	return r
}

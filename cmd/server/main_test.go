package main

import (
	"bytes"
	"io"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"
)

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
			if !called.Load() || response.Code != http.StatusOK || !strings.Contains(response.Body.String(), `"raw_text":"OAuth"`) {
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

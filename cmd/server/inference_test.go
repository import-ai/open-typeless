package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestAudioCPPRecognition(t *testing.T) {
	inference := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "POST" || r.URL.Path != "/v1/audio/transcriptions" {
			t.Errorf("unexpected route: %s", r.URL)
		}
		if err := r.ParseMultipartForm(1 << 20); err != nil {
			t.Error(err)
			return
		}
		defer r.MultipartForm.RemoveAll()
		if r.FormValue("model") != "r2t2-asr" || r.FormValue("prompt") != "OAuth\n语音" {
			t.Errorf("unexpected form: %+v", r.Form)
		}
		if _, present := r.MultipartForm.Value["language"]; present {
			t.Error("auto language must be omitted")
		}
		f, h, err := r.FormFile("file")
		if err != nil {
			t.Error(err)
			return
		}
		defer f.Close()
		audio, _ := io.ReadAll(f)
		if !bytes.Equal(audio, []byte{0, 1, 128, 255}) || h.Filename != "recording.wav" {
			t.Error("audio or filename changed")
		}
		io.WriteString(w, `{"text":"OAuth","timing":{"audio_duration_ms":4203.94,"wall_ms":400}}`)
	}))
	defer inference.Close()
	s := &server{inferenceURL: inference.URL, inferenceProtocol: "audiocpp", inferenceModel: "r2t2-asr", client: inference.Client(), maxBytes: 12 << 20}
	w := httptest.NewRecorder()
	s.recognize(w, recognitionRequest(t, "OAuth\n语音"))
	var result recognitionResponse
	if err := json.Unmarshal(w.Body.Bytes(), &result); err != nil {
		t.Fatal(err)
	}
	if w.Code != 200 || result.RawText != "OAuth" || result.PolishedText != "OAuth" || result.DurationMS != 4204 || result.Language != "auto" {
		t.Fatalf("unexpected response: %d %+v", w.Code, result)
	}
}

func TestAudioCPPForwardsExplicitLanguage(t *testing.T) {
	s := &server{inferenceURL: "http://localhost", inferenceProtocol: "audiocpp", inferenceModel: "r2t2-asr"}
	r, err := s.inferenceRequest(context.Background(), []byte("audio"), "clip.mp3", "en", "")
	if err != nil {
		t.Fatal(err)
	}
	if err := r.ParseMultipartForm(1 << 20); err != nil {
		t.Fatal(err)
	}
	defer r.MultipartForm.RemoveAll()
	if r.FormValue("language") != "en" {
		t.Fatal("explicit language not forwarded")
	}
}

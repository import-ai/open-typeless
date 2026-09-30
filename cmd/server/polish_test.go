package main

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestRecognitionPolishesAfterASR(t *testing.T) {
	var asrDone atomic.Bool
	inference := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "" {
			t.Error("backend credential forwarded to ASR")
		}
		asrDone.Store(true)
		io.WriteString(w, `{"raw_text":"嗯明天下午三点开会","language":"zh","audio_duration_ms":1234}`)
	}))
	defer inference.Close()
	llm := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !asrDone.Load() || r.Method != "POST" || r.URL.Path != "/v1/chat/completions" || r.Header.Get("Authorization") != "Bearer test-key" {
			t.Errorf("unexpected LLM request: %s %s", r.Method, r.URL)
		}
		var request polishRequest
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
			t.Error(err)
		}
		if request.Model != "test-model" || request.MaxTokens != 512 || request.Stream || request.ChatTemplateKwargs.EnableThinking {
			t.Errorf("incorrect model parameters: %+v", request)
		}
		if len(request.Messages) != 8 {
			t.Fatalf("expected system + three pairs + transcript, got %d messages", len(request.Messages))
		}
		for i, role := range []string{"system", "user", "assistant", "user", "assistant", "user", "assistant", "user"} {
			if request.Messages[i].Role != role {
				t.Errorf("incorrect role at %d", i)
			}
			if role == "user" && (!strings.HasPrefix(request.Messages[i].Content, "<raw_asr_result>\n") || !strings.HasSuffix(request.Messages[i].Content, "\n</raw_asr_result>")) {
				t.Errorf("user message %d did not use the template", i)
			}
		}
		if request.Messages[7].Content != "<raw_asr_result>\n嗯明天下午三点开会\n</raw_asr_result>" {
			t.Errorf("unexpected rendered transcript: %q", request.Messages[7].Content)
		}
		io.WriteString(w, `{"choices":[{"message":{"content":" 明天下午三点开会。 ","reasoning_content":"never paste this"},"finish_reason":"stop"}]}`)
	}))
	defer llm.Close()
	s := &server{inferenceURL: inference.URL, client: inference.Client(), maxBytes: 12 << 20,
		polisher: &polisher{baseURL: llm.URL + "/v1", model: "test-model", apiKey: "test-key", timeout: time.Second, client: llm.Client()}}
	w := httptest.NewRecorder()
	r := recognitionRequest(t, "OAuth")
	r.Header.Set("Authorization", "Bearer backend-test-key")
	s.handler("backend-test-key").ServeHTTP(w, r)
	var result recognitionResponse
	if err := json.Unmarshal(w.Body.Bytes(), &result); err != nil {
		t.Fatal(err)
	}
	if w.Code != 200 || result.RawText != "嗯明天下午三点开会" || result.PolishedText != "明天下午三点开会。" || result.Language != "zh" || result.DurationMS != 1234 {
		t.Fatalf("unexpected recognition: %d %+v", w.Code, result)
	}
}

func TestRecognitionFallsBackOnPolishingFailure(t *testing.T) {
	for _, tc := range []struct {
		name, body string
		status     int
		timeout    bool
	}{
		{"http_error", `secret upstream error`, 500, false},
		{"malformed_json", `{`, 200, false},
		{"missing_choice", `{"choices":[]}`, 200, false},
		{"empty", `{"choices":[{"message":{"content":" "},"finish_reason":"stop"}]}`, 200, false},
		{"reasoning_only", `{"choices":[{"message":{"reasoning_content":"thinking"},"finish_reason":"stop"}]}`, 200, false},
		{"truncated", `{"choices":[{"message":{"content":"partial"},"finish_reason":"length"}]}`, 200, false},
		{"missing_finish", `{"choices":[{"message":{"content":"partial"}}]}`, 200, false},
		{"refusal", `{"choices":[{"message":{"content":"refused","refusal":"no"},"finish_reason":"stop"}]}`, 200, false},
		{"tool_call", `{"choices":[{"message":{"content":"call","tool_calls":[{}]},"finish_reason":"tool_calls"}]}`, 200, false},
		{"oversized", strings.Repeat("a", (2<<20)+1), 200, false},
		{"timeout", "", 200, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			inference := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				io.WriteString(w, `{"raw_text":"原文","language":"zh","audio_duration_ms":2000}`)
			}))
			defer inference.Close()
			llm := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				io.Copy(io.Discard, r.Body)
				if tc.timeout {
					<-r.Context().Done()
					return
				}
				w.WriteHeader(tc.status)
				io.WriteString(w, tc.body)
			}))
			defer llm.Close()
			s := &server{inferenceURL: inference.URL, client: inference.Client(), maxBytes: 12 << 20,
				polisher: &polisher{baseURL: llm.URL, timeout: time.Second, client: llm.Client()}}
			if tc.timeout {
				s.polisher.timeout = 20 * time.Millisecond
			}
			w := httptest.NewRecorder()
			s.recognize(w, recognitionRequest(t, ""))
			var result recognitionResponse
			json.Unmarshal(w.Body.Bytes(), &result)
			if w.Code != 200 || result.RawText != "原文" || result.PolishedText != "原文" || result.DurationMS != 2000 {
				t.Fatalf("fallback failed: %d %s", w.Code, w.Body)
			}
		})
	}
}

func TestRecognitionSkipsPolishing(t *testing.T) {
	for _, tc := range []struct {
		name, body string
		status     int
	}{
		{"empty", `{"raw_text":""}`, 200},
		{"whitespace", `{"raw_text":"  "}`, 200},
		{"asr_failure", `failed`, 503},
		{"asr_invalid", `{`, 502},
	} {
		t.Run(tc.name, func(t *testing.T) {
			inference := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if tc.status == 503 {
					w.WriteHeader(503)
				}
				io.WriteString(w, tc.body)
			}))
			defer inference.Close()
			llm := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { t.Error("LLM must not be called") }))
			defer llm.Close()
			s := &server{inferenceURL: inference.URL, client: inference.Client(), maxBytes: 12 << 20,
				polisher: &polisher{baseURL: llm.URL, timeout: time.Second, client: llm.Client()}}
			w := httptest.NewRecorder()
			s.recognize(w, recognitionRequest(t, ""))
			if w.Code != tc.status {
				t.Fatalf("got status %d", w.Code)
			}
		})
	}
}

func TestRecognitionCancellationStopsPolishing(t *testing.T) {
	started, stopped := make(chan struct{}), make(chan struct{})
	inference := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { io.WriteString(w, `{"raw_text":"raw"}`) }))
	defer inference.Close()
	llm := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		io.Copy(io.Discard, r.Body)
		close(started)
		<-r.Context().Done()
		close(stopped)
	}))
	defer llm.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	s := &server{inferenceURL: inference.URL, client: inference.Client(), maxBytes: 12 << 20,
		polisher: &polisher{baseURL: llm.URL, timeout: time.Second, client: llm.Client()}}
	w := httptest.NewRecorder()
	r := recognitionRequest(t, "").WithContext(ctx)
	done := make(chan struct{})
	go func() { s.recognize(w, r); close(done) }()
	select {
	case <-started:
	case <-time.After(2 * time.Second):
		t.Fatal("LLM not started")
	}
	cancel()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("recognition not cancelled")
	}
	select {
	case <-stopped:
	case <-time.After(time.Second):
		t.Fatal("upstream not cancelled")
	}
	if w.Body.Len() != 0 {
		t.Fatalf("cancelled request returned text: %s", w.Body)
	}
}

func TestPolisherConfiguration(t *testing.T) {
	t.Setenv("LLM_BASE_URL", "")
	t.Setenv("LLM_PROMPT_FILE", "")
	p, err := polisherFromEnv()
	if p != nil || err != nil {
		t.Fatal("polishing should be disabled")
	}
	t.Setenv("LLM_BASE_URL", "http://localhost:18081/v1/")
	t.Setenv("LLM_TIMEOUT", "")
	t.Setenv("LLM_MODEL", "")
	p, err = polisherFromEnv()
	if err != nil || p.baseURL != "http://localhost:18081/v1" || p.timeout != 10*time.Second || p.model != "minicpm5-2b-q4" || len(p.prompt.Samples) != 3 {
		t.Fatalf("incorrect defaults: %+v %v", p, err)
	}
	promptPath := filepath.Join(t.TempDir(), "prompt.json")
	if err := os.WriteFile(promptPath, []byte(`{"system":"custom","template":"<input>${query}</input>","samples":[]}`), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("LLM_PROMPT_FILE", promptPath)
	p, err = polisherFromEnv()
	if err != nil || len(p.prompt.Samples) != 0 || p.prompt.System != "custom" || p.prompt.Template != "<input>${query}</input>" {
		t.Fatalf("custom prompt was not loaded: %+v %v", p, err)
	}
	for _, invalid := range []string{"{", "null", "{}", `[{"role":"system","content":"legacy"}]`, `{"template":"no placeholder"}`, `{"template":"${query}","samples":[{"query":42}]}`} {
		if err := os.WriteFile(promptPath, []byte(invalid), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, err := polisherFromEnv(); err == nil {
			t.Fatalf("invalid custom prompt accepted: %s", invalid)
		}
	}
	t.Setenv("LLM_TIMEOUT", "-1s")
	if _, err := polisherFromEnv(); err == nil {
		t.Fatal("negative timeout accepted")
	}
	t.Setenv("LLM_TIMEOUT", "1s")
	for _, base := range []string{"ftp://localhost/v1", "http://user:secret@localhost/v1", "http://localhost/v1?key=secret", ":invalid"} {
		t.Setenv("LLM_BASE_URL", base)
		if _, err := polisherFromEnv(); err == nil {
			t.Fatal("invalid base URL accepted")
		}
	}
}

func TestPolisherCustomTemplate(t *testing.T) {
	want := []chatMessage{
		{Role: "system", Content: "Instructions ${query}"},
		{Role: "user", Content: "[sample]\nsample"},
		{Role: "assistant", Content: "Answer ${query}"},
		{Role: "user", Content: "[raw ${query}\n<&>]\nraw ${query}\n<&>"},
	}
	llm := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request polishRequest
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
			t.Error(err)
		}
		if !reflect.DeepEqual(request.Messages, want) {
			t.Errorf("unexpected messages: %#v", request.Messages)
		}
		io.WriteString(w, `{"choices":[{"message":{"content":"polished"},"finish_reason":"stop"}]}`)
	}))
	defer llm.Close()
	path := filepath.Join(t.TempDir(), "prompt.json")
	if err := os.WriteFile(path, []byte(`{"system":"Instructions ${query}","template":"[${query}]\n${query}","samples":[{"query":"sample","answer":"Answer ${query}"}]}`), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("LLM_BASE_URL", llm.URL)
	t.Setenv("LLM_TIMEOUT", "1s")
	t.Setenv("LLM_PROMPT_FILE", path)
	p, err := polisherFromEnv()
	if err != nil {
		t.Fatal(err)
	}
	// The startup snapshot survives file changes, and requests never accumulate messages.
	if err := os.WriteFile(path, []byte("{"), 0o600); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		if text, err := p.polish(context.Background(), "raw ${query}\n<&>"); err != nil || text != "polished" {
			t.Fatalf("polishing failed: %q %v", text, err)
		}
	}
}

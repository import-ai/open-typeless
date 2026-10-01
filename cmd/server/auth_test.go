package main

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestAPIKeyProtectsAllRoutes(t *testing.T) {
	s := &server{maxBytes: 12 << 20}
	for _, path := range []string{"/api/v1/health", "/api/v1/recognitions", "/api/v1/recognitions/stream"} {
		for _, tc := range []struct {
			name    string
			headers []string
			valid   bool
		}{
			{"missing", nil, false},
			{"wrong", []string{"Bearer wrong-key"}, false},
			{"empty", []string{"Bearer "}, false},
			{"wrong_scheme", []string{"Basic test-key"}, false},
			{"extra_token", []string{"Bearer test-key extra"}, false},
			{"duplicate", []string{"Bearer test-key", "Bearer test-key"}, false},
			{"valid", []string{"Bearer test-key"}, true},
			{"case_insensitive_scheme", []string{"bearer test-key"}, true},
		} {
			t.Run(path+"/"+tc.name, func(t *testing.T) {
				r := httptest.NewRequest(http.MethodPost, path+"?api_key=test-key", nil)
				for _, value := range tc.headers {
					r.Header.Add("Authorization", value)
				}
				w := httptest.NewRecorder()
				s.handler("test-key").ServeHTTP(w, r)
				if tc.valid {
					expected := http.StatusOK
					if path == "/api/v1/recognitions" {
						expected = http.StatusBadRequest
					}
					if path == "/api/v1/recognitions/stream" {
						expected = http.StatusNotImplemented
					}
					if w.Code != expected {
						t.Fatalf("authenticated request got %d", w.Code)
					}
				} else {
					if w.Code != http.StatusUnauthorized || w.Header().Get("WWW-Authenticate") == "" {
						t.Fatalf("unauthenticated request got %d", w.Code)
					}
					if strings.Contains(w.Body.String(), "test-key") || strings.Contains(w.Body.String(), "wrong-key") {
						t.Fatal("response exposed a credential")
					}
				}
			})
		}
	}
}

type unreadableBody struct{ t *testing.T }

func (b unreadableBody) Read([]byte) (int, error) {
	b.t.Fatal("unauthenticated body was read")
	return 0, nil
}
func (b unreadableBody) Close() error { return nil }

func TestAuthenticationRunsBeforeReadingAudio(t *testing.T) {
	r := httptest.NewRequest(http.MethodPost, "/api/v1/recognitions", nil)
	r.Body = unreadableBody{t}
	w := httptest.NewRecorder()
	(&server{}).handler("test-key").ServeHTTP(w, r)
	if w.Code != 401 {
		t.Fatalf("got %d", w.Code)
	}
}

func TestEmptyKeyFailsClosedAndPreflightAllowsAuthorization(t *testing.T) {
	handler := (&server{}).handler("")
	r := httptest.NewRequest(http.MethodGet, "/api/v1/health", nil)
	r.Header.Set("Authorization", "Bearer ")
	w := httptest.NewRecorder()
	handler.ServeHTTP(w, r)
	if w.Code != 401 {
		t.Fatal("empty configured key allowed anonymous access")
	}
	r = httptest.NewRequest(http.MethodOptions, "/api/v1/recognitions", nil)
	r.Header.Set("Origin", "http://localhost:5173")
	r.Header.Set("Access-Control-Request-Headers", "authorization")
	w = httptest.NewRecorder()
	handler.ServeHTTP(w, r)
	if w.Code != 204 || !strings.Contains(w.Header().Get("Access-Control-Allow-Headers"), "Authorization") || !strings.Contains(w.Header().Get("Access-Control-Allow-Methods"), "POST") {
		t.Fatal("preflight failed")
	}
}

func TestBackendAPIKeyConfiguration(t *testing.T) {
	for _, key := range []string{"", " ", "key with spaces", "key\n", "密钥"} {
		t.Setenv("BACKEND_API_KEY", key)
		if _, err := backendAPIKeyFromEnv(); err == nil {
			t.Fatal("invalid configuration accepted")
		}
	}
	t.Setenv("BACKEND_API_KEY", "test-key_123")
	key, err := backendAPIKeyFromEnv()
	if err != nil || key != "test-key_123" {
		t.Fatal("valid configuration rejected")
	}
}

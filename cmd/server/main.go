package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/lucienshui/open-typeless/internal/buildinfo"
)

type server struct {
	inferenceURL         string
	inferenceProtocol    string
	inferenceStreamModel string
	inferenceModel       string
	polisher             *polisher
	client               *http.Client
	maxBytes             int64
}

type recognitionResponse struct {
	RawText      string `json:"raw_text"`
	PolishedText string `json:"polished_text"`
	Language     string `json:"language"`
	DurationMS   int    `json:"duration_ms"`
}

func main() {
	apiKey, err := backendAPIKeyFromEnv()
	if err != nil {
		log.Fatal(err)
	}
	polish, err := polisherFromEnv()
	if err != nil {
		log.Fatal(err)
	}
	s := &server{
		inferenceURL:         strings.TrimRight(env("INFERENCE_URL", "http://localhost:18080"), "/"),
		inferenceProtocol:    env("INFERENCE_PROTOCOL", "legacy"),
		inferenceStreamModel: strings.TrimSpace(os.Getenv("INFERENCE_STREAM_MODEL")),
		inferenceModel:       env("INFERENCE_MODEL", "r2t2-asr"),
		polisher:             polish,
		client:               &http.Client{Timeout: 45 * time.Second},
		maxBytes:             envInt64("MAX_AUDIO_BYTES", 12<<20),
	}
	if s.inferenceProtocol != "legacy" && s.inferenceProtocol != "audiocpp" {
		log.Fatal("INFERENCE_PROTOCOL must be legacy or audiocpp")
	}
	if s.inferenceStreamModel != "" && s.inferenceProtocol != "audiocpp" {
		log.Fatal("INFERENCE_STREAM_MODEL requires INFERENCE_PROTOCOL=audiocpp")
	}
	addr := env("HTTP_ADDR", ":8080")
	log.Printf("open-typeless business server listening on %s, inference=%s", addr, s.inferenceURL)
	log.Fatal(http.ListenAndServe(addr, s.handler(apiKey)))
}

func (s *server) handler(apiKey string) http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/api/v1/health", s.health)
	mux.HandleFunc("/api/v1/recognitions", s.recognize)
	mux.HandleFunc("/api/v1/recognitions/stream", s.recognizeStream)
	return logging(cors(requireAPIKey(apiKey, mux)))
}

func (s *server) health(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{"status": "ok", "version": buildinfo.Version,
		"capabilities": map[string]bool{"streaming_asr": s.inferenceStreamModel != ""}})
}

func (s *server) recognize(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	if r.ContentLength > s.maxBytes+1<<20 {
		writeError(w, http.StatusRequestEntityTooLarge, "audio file is too large")
		return
	}
	if err := r.ParseMultipartForm(s.maxBytes); err != nil {
		writeError(w, http.StatusBadRequest, "multipart form is invalid")
		return
	}
	defer r.MultipartForm.RemoveAll()
	file, header, err := r.FormFile("audio")
	if err != nil {
		writeError(w, http.StatusBadRequest, "audio field is required")
		return
	}
	defer file.Close()
	audio, err := io.ReadAll(io.LimitReader(file, s.maxBytes+1))
	if err != nil || int64(len(audio)) > s.maxBytes {
		writeError(w, http.StatusRequestEntityTooLarge, "audio file is too large")
		return
	}
	language := r.FormValue("language")
	if language == "" {
		language = "auto"
	}
	contextText := r.FormValue("hotwords")
	if len(contextText) > 1000 {
		writeError(w, http.StatusBadRequest, "hotwords is too long")
		return
	}

	ctx, cancel := context.WithTimeout(r.Context(), 40*time.Second)
	defer cancel()
	req, err := s.inferenceRequest(ctx, audio, header.Filename, language, contextText)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to build inference request")
		return
	}
	resp, err := s.client.Do(req)
	if err != nil {
		writeError(w, http.StatusBadGateway, "inference server unavailable")
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(io.LimitReader(resp.Body, 2<<20))
	if resp.StatusCode != http.StatusOK {
		writeError(w, resp.StatusCode, fmt.Sprintf("inference server returned %d: %s", resp.StatusCode, strings.TrimSpace(string(body))))
		return
	}
	var asr struct {
		RawText         string  `json:"raw_text"`
		Language        string  `json:"language"`
		AudioDurationMS int     `json:"audio_duration_ms"`
		Text            *string `json:"text"`
		Timing          struct {
			AudioDurationMS float64 `json:"audio_duration_ms"`
		} `json:"timing"`
	}
	if err := json.Unmarshal(body, &asr); err != nil {
		writeError(w, http.StatusBadGateway, "invalid inference response")
		return
	}
	if s.inferenceProtocol == "audiocpp" {
		if asr.Text == nil {
			writeError(w, http.StatusBadGateway, "invalid inference response")
			return
		}
		asr.RawText = *asr.Text
		asr.AudioDurationMS = int(asr.Timing.AudioDurationMS + 0.5)
		if asr.Language == "" {
			asr.Language = language
		}
	}
	result := s.polishRecognition(r.Context(), asr.RawText, asr.Language, asr.AudioDurationMS)
	if r.Context().Err() == nil {
		writeJSON(w, http.StatusOK, result)
	}
}

func env(k, fallback string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return fallback
}
func envInt64(k string, fallback int64) int64 {
	v, err := strconv.ParseInt(os.Getenv(k), 10, 64)
	if err != nil || v <= 0 {
		return fallback
	}
	return v
}
func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}
func writeError(w http.ResponseWriter, status int, message string) {
	writeJSON(w, status, map[string]string{"error": message})
}
func cors(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
		w.Header().Set("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		next.ServeHTTP(w, r)
	})
}
func logging(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		next.ServeHTTP(w, r)
		log.Printf("%s %s %s", r.Method, r.URL.Path, time.Since(start))
	})
}

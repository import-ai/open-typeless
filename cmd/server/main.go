package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"
)

type server struct {
	inferenceURL string
	client       *http.Client
	maxBytes     int64
}

type recognitionResponse struct {
	RawText      string `json:"raw_text"`
	PolishedText string `json:"polished_text"`
	Language     string `json:"language"`
	DurationMS   int    `json:"duration_ms"`
}

func main() {
	s := &server{
		inferenceURL: strings.TrimRight(env("INFERENCE_URL", "http://localhost:18080"), "/"),
		client:       &http.Client{Timeout: 45 * time.Second},
		maxBytes:     envInt64("MAX_AUDIO_BYTES", 12<<20),
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", s.health)
	mux.HandleFunc("/v1/recognitions", s.recognize)
	addr := env("HTTP_ADDR", ":8080")
	log.Printf("open-typeless business server listening on %s, inference=%s", addr, s.inferenceURL)
	log.Fatal(http.ListenAndServe(addr, logging(cors(mux))))
}

func (s *server) health(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
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
	file, _, err := r.FormFile("audio")
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

	queryValues := url.Values{"language": {language}}
	if contextText != "" {
		queryValues.Set("context", contextText)
	}
	query := "?" + queryValues.Encode()
	ctx, cancel := context.WithTimeout(r.Context(), 40*time.Second)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, s.inferenceURL+"/transcribe"+query, strings.NewReader(string(audio)))
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to build inference request")
		return
	}
	req.Header.Set("Content-Type", "application/octet-stream")
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
		RawText         string `json:"raw_text"`
		Language        string `json:"language"`
		AudioDurationMS int    `json:"audio_duration_ms"`
		ProcessingMS    int    `json:"processing_ms"`
	}
	if err := json.Unmarshal(body, &asr); err != nil {
		writeError(w, http.StatusBadGateway, "invalid inference response")
		return
	}
	writeJSON(w, http.StatusOK, recognitionResponse{RawText: asr.RawText, PolishedText: asr.RawText, Language: asr.Language, DurationMS: asr.AudioDurationMS})
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
		w.Header().Set("Access-Control-Allow-Headers", "Content-Type")
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

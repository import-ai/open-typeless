package main

import (
	"crypto/sha256"
	"crypto/subtle"
	"errors"
	"net/http"
	"os"
	"strings"
)

func backendAPIKeyFromEnv() (string, error) {
	key := os.Getenv("BACKEND_API_KEY")
	if key == "" {
		return "", errors.New("BACKEND_API_KEY is required")
	}
	for _, c := range key {
		if c < 33 || c > 126 {
			return "", errors.New("BACKEND_API_KEY must contain only printable ASCII without spaces")
		}
	}
	return key, nil
}

func requireAPIKey(key string, next http.Handler) http.Handler {
	expected := sha256.Sum256([]byte(key))
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		values := r.Header.Values("Authorization")
		scheme, token, found := strings.Cut(r.Header.Get("Authorization"), " ")
		actual := sha256.Sum256([]byte(token))
		if key == "" || len(values) != 1 || !found || !strings.EqualFold(scheme, "Bearer") || subtle.ConstantTimeCompare(actual[:], expected[:]) != 1 {
			w.Header().Set("WWW-Authenticate", `Bearer realm="open-typeless"`)
			writeError(w, http.StatusUnauthorized, "invalid or missing API key")
			return
		}
		next.ServeHTTP(w, r)
	})
}

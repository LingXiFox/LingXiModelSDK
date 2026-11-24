package main

import (
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"
)

func isLoopbackAddr(addr string) bool {
	host, _, err := net.SplitHostPort(addr)
	if err != nil {
		host = addr
	}
	host = strings.Trim(host, "[]")
	if strings.EqualFold(host, "localhost") {
		return true
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}

// ValidateStartupSecurity enforces fail-closed security for non-loopback binds.
func ValidateStartupSecurity(listenAddr, authToken string, allowHosts []string) error {
	if isLoopbackAddr(listenAddr) {
		return nil
	}
	if strings.TrimSpace(authToken) == "" {
		return errors.New("non-loopback listen address requires -auth-token")
	}
	if len(allowHosts) == 0 {
		return errors.New("non-loopback listen address requires -allow-hosts egress allowlist")
	}
	return nil
}

// CheckEgressAllowed prevents arbitrary URL proxying / SSRF.
func CheckEgressAllowed(rawURL string, allowHosts []string, allowLoopbackEgress bool) error {
	u, err := url.Parse(rawURL)
	if err != nil || u.Hostname() == "" {
		return fmt.Errorf("invalid target URL %q", rawURL)
	}
	host := strings.ToLower(u.Hostname())
	if allowLoopbackEgress && (host == "127.0.0.1" || host == "localhost" || host == "::1") {
		return nil
	}
	if len(allowHosts) == 0 {
		// When allowHosts is empty on loopback dev mode, only HTTPS registry endpoints are permitted
		// unless allowLoopbackEgress was explicitly enabled.
		if u.Scheme != "https" {
			return fmt.Errorf("egress host %q blocked (non-HTTPS without allowlist)", host)
		}
		return nil
	}
	for _, allowed := range allowHosts {
		a := strings.ToLower(strings.TrimSpace(allowed))
		if a == "" {
			continue
		}
		if host == a || (strings.HasPrefix(a, "*.") && strings.HasSuffix(host, a[1:])) {
			return nil
		}
	}
	return fmt.Errorf("egress host %q is not in -allow-hosts", host)
}

type bucket struct {
	tokens float64
	last   time.Time
}

// TokenBucketLimiter implements per-key rate limiting.
type TokenBucketLimiter struct {
	rps   float64
	burst float64
	mu    sync.Mutex
	peers map[string]*bucket
}

func NewTokenBucketLimiter(rps, burst float64) *TokenBucketLimiter {
	if rps <= 0 {
		rps = 20
	}
	if burst < 1 {
		burst = 40
	}
	return &TokenBucketLimiter{
		rps:   rps,
		burst: burst,
		peers: make(map[string]*bucket),
	}
}

func (l *TokenBucketLimiter) Allow(key string) bool {
	now := time.Now()
	l.mu.Lock()
	defer l.mu.Unlock()
	b, ok := l.peers[key]
	if !ok {
		l.peers[key] = &bucket{tokens: l.burst - 1, last: now}
		return true
	}
	elapsed := now.Sub(b.last).Seconds()
	b.last = now
	b.tokens += elapsed * l.rps
	if b.tokens > l.burst {
		b.tokens = l.burst
	}
	if b.tokens < 1 {
		return false
	}
	b.tokens -= 1
	return true
}

func clientKey(r *http.Request) string {
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}
	return host
}

package main

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"
)

var cgnatRange = func() *net.IPNet {
	_, n, _ := net.ParseCIDR("100.64.0.0/10")
	return n
}()

// IsPrivateOrReservedIP returns true for loopback, RFC1918, link-local/metadata, ULA, CGNAT, and multicast IPs.
func IsPrivateOrReservedIP(ip net.IP) bool {
	if ip == nil {
		return true
	}
	return ip.IsLoopback() ||
		ip.IsPrivate() ||
		ip.IsLinkLocalUnicast() ||
		ip.IsLinkLocalMulticast() ||
		ip.IsMulticast() ||
		ip.IsUnspecified() ||
		cgnatRange.Contains(ip)
}

// NewSafeHTTPClient builds an HTTP client that blocks redirects and validates dialed IPs against SSRF.
func NewSafeHTTPClient(allowLoopback bool) *http.Client {
	dialer := &net.Dialer{Timeout: 10 * time.Second, KeepAlive: 30 * time.Second}
	tr := &http.Transport{
		MaxIdleConns:        128,
		MaxIdleConnsPerHost: 32,
		IdleConnTimeout:     90 * time.Second,
		DialContext: func(ctx context.Context, network, addr string) (net.Conn, error) {
			host, port, err := net.SplitHostPort(addr)
			if err != nil {
				return nil, err
			}
			ips, err := net.DefaultResolver.LookupIP(ctx, "ip", host)
			if err != nil {
				return nil, err
			}
			for _, ip := range ips {
				if allowLoopback && ip.IsLoopback() {
					continue
				}
				if IsPrivateOrReservedIP(ip) {
					return nil, fmt.Errorf("SSRF blocked dial to reserved IP %s (%s)", ip.String(), host)
				}
			}
			return dialer.DialContext(ctx, network, net.JoinHostPort(ips[0].String(), port))
		},
	}
	return &http.Client{
		Timeout:   60 * time.Second,
		Transport: tr,
		CheckRedirect: func(*http.Request, []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}
}

// RedactSecret masks API keys and bearer tokens before logging or error emission.
func RedactSecret(s string) string {
	if len(s) <= 8 {
		return "***"
	}
	return s[:4] + "..." + s[len(s)-2:]
}

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
	if ip := net.ParseIP(host); ip != nil && IsPrivateOrReservedIP(ip) {
		return fmt.Errorf("egress IP %q blocked (private/reserved/metadata)", host)
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

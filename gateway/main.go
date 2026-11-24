package main

import (
	"context"
	"errors"
	"flag"
	"log"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/LingXiFox/LingXiModelSDK/sdk/go/native"
)

func main() {
	listen := flag.String("listen", "127.0.0.1:18090", "Gateway listen address (loopback by default)")
	regPath := flag.String("registry", "../registry/registry.json", "Path to declarative provider registry JSON")
	authToken := flag.String("auth-token", "", "Bearer token required for gateway requests (mandatory for non-loopback)")
	allowHostsRaw := flag.String("allow-hosts", "", "Comma-separated egress host allowlist (mandatory for non-loopback)")
	allowLoopbackEgress := flag.Bool("allow-loopback-egress", false, "Allow loopback HTTP egress (for local testing)")
	rps := flag.Float64("rate-limit-rps", 20, "Per-client rate limit requests per second")
	burst := flag.Float64("rate-limit-burst", 40, "Per-client rate limit burst")
	flag.Parse()

	var allowHosts []string
	for _, h := range strings.Split(*allowHostsRaw, ",") {
		if trimmed := strings.TrimSpace(h); trimmed != "" {
			allowHosts = append(allowHosts, trimmed)
		}
	}

	if err := ValidateStartupSecurity(*listen, *authToken, allowHosts); err != nil {
		log.Fatalf("gateway security policy rejected configuration: %v", err)
	}

	var reg *native.Registry
	if *regPath != "" {
		b, err := os.ReadFile(*regPath)
		if err != nil {
			log.Fatalf("failed to read registry %s: %v", *regPath, err)
		}
		reg, err = native.ParseRegistry(b)
		if err != nil {
			log.Fatalf("failed to parse registry %s: %v", *regPath, err)
		}
	}

	gw := NewGateway(GatewayConfig{
		ListenAddr:          *listen,
		AuthToken:           *authToken,
		AllowHosts:          allowHosts,
		AllowLoopbackEgress: *allowLoopbackEgress,
		RateLimitRPS:        *rps,
		RateLimitBurst:      *burst,
	}, reg)

	srv := &http.Server{
		Addr:              *listen,
		Handler:           gw.Handler(),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       30 * time.Second,
		WriteTimeout:      120 * time.Second,
		IdleTimeout:       60 * time.Second,
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go func() {
		<-ctx.Done()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		_ = srv.Shutdown(shutdownCtx)
	}()

	log.Printf("LingXi Gateway listening on %s", *listen)
	if err := srv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Fatal(err)
	}
}

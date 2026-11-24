package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"
)

func main() {
	addr := flag.String("listen", "127.0.0.1:8080", "HTTP listen address")
	dir := flag.String("data-dir", "./data", "persistent catalog directory")
	source := flag.String("source", defaultSourceURL, "trusted upstream catalog URL")
	every := flag.Duration("sync-interval", time.Hour, "background sync interval (0 disables)")
	once := flag.Bool("sync-once", false, "refresh catalog once and exit")
	shrink := flag.Bool("allow-shrink", false, "permit >30% shrink (sync-once only)")
	minRatio := flag.Float64("min-model-ratio", defaultMinRatio, "minimum retained model/provider ratio vs previous snapshot")
	checkOnly := flag.Bool("check", false, "validate existing models.json in data-dir and exit")
	adminAddr := flag.String("admin-listen", "", "optional loopback-only admin HTTP address for POST /admin/sync")
	syncToken := flag.String("sync-token", "", "optional bearer token for -admin-listen POST /admin/sync")
	flag.Parse()

	if *every < 0 || (*shrink && !*once) || *minRatio <= 0 || *minRatio > 1 {
		log.Fatal("invalid sync options")
	}

	if *checkOnly {
		manifest, err := CheckSnapshot(*dir)
		if err != nil {
			log.Fatalf("check failed: %v", err)
		}
		out, err := json.MarshalIndent(manifest, "", "  ")
		if err != nil {
			log.Fatal(err)
		}
		fmt.Println(string(out))
		return
	}

	store := newStore(*dir, *minRatio)
	if err := store.load(); err != nil {
		if !errors.Is(err, os.ErrNotExist) {
			log.Printf("cached catalog rejected: %v", err)
		}
	} else if snap := store.snapshot(); snap != nil {
		log.Printf("loaded cached catalog: revision=%s providers=%d models=%d",
			snap.Catalog.Revision, snap.Catalog.TotalProviders, snap.Catalog.TotalModels)
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	client := &http.Client{Timeout: 60 * time.Second}
	syncNow := func(syncCtx context.Context) error {
		return store.sync(syncCtx, client, *source, *shrink)
	}

	if *once {
		if err := syncNow(ctx); err != nil {
			log.Fatal(err)
		}
		snap := store.snapshot()
		log.Printf("sync-once complete: revision=%s providers=%d models=%d",
			snap.Catalog.Revision, snap.Catalog.TotalProviders, snap.Catalog.TotalModels)
		return
	}

	manualSyncCh := registerManualSyncSignal(ctx)

	syncDone := make(chan struct{})
	go func() {
		defer close(syncDone)
		runSync := func(reason string) {
			if err := syncNow(ctx); err != nil {
				log.Printf("sync (%s) failed: %v", reason, err)
				return
			}
			if snap := store.snapshot(); snap != nil {
				log.Printf("catalog updated (%s): revision=%s providers=%d models=%d",
					reason, snap.Catalog.Revision, snap.Catalog.TotalProviders, snap.Catalog.TotalModels)
			}
		}

		if *every > 0 {
			runSync("startup")
			ticker := time.NewTicker(*every)
			defer ticker.Stop()
			for {
				select {
				case <-ctx.Done():
					return
				case <-ticker.C:
					runSync("scheduled")
				case <-manualSyncCh:
					runSync("sighup")
				}
			}
		} else {
			for {
				select {
				case <-ctx.Done():
					return
				case <-manualSyncCh:
					runSync("sighup")
				}
			}
		}
	}()

	var adminSrv *http.Server
	if *adminAddr != "" {
		adminSrv = &http.Server{
			Addr:              *adminAddr,
			Handler:           adminHandler(store, syncNow, *syncToken),
			ReadHeaderTimeout: 5 * time.Second,
			ReadTimeout:       15 * time.Second,
			WriteTimeout:      120 * time.Second,
			IdleTimeout:       60 * time.Second,
			MaxHeaderBytes:    1 << 16,
		}
		go func() {
			log.Printf("admin sync endpoint listening on %s", *adminAddr)
			if err := adminSrv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
				log.Printf("admin server error: %v", err)
			}
		}()
	}

	srv := &http.Server{
		Addr:              *addr,
		Handler:           publicHandler(store),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       15 * time.Second,
		WriteTimeout:      30 * time.Second,
		IdleTimeout:       60 * time.Second,
		MaxHeaderBytes:    1 << 20,
	}

	go func() {
		<-ctx.Done()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		_ = srv.Shutdown(shutdownCtx)
		if adminSrv != nil {
			_ = adminSrv.Shutdown(shutdownCtx)
		}
	}()

	log.Printf("LingXi Models listening on %s", *addr)
	if err := srv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Fatal(err)
	}
	stop()
	<-syncDone
}

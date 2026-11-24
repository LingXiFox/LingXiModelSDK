//go:build !windows

package main

import (
	"context"
	"os"
	"os/signal"
	"syscall"
)

func registerManualSyncSignal(ctx context.Context) <-chan struct{} {
	ch := make(chan struct{}, 1)
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGHUP)
	go func() {
		defer signal.Stop(sig)
		for {
			select {
			case <-ctx.Done():
				return
			case <-sig:
				select {
				case ch <- struct{}{}:
				default:
				}
			}
		}
	}()
	return ch
}

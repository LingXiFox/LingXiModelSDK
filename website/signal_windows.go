//go:build windows

package main

import "context"

func registerManualSyncSignal(ctx context.Context) <-chan struct{} {
	return nil
}

// Run with scripts/check-ios-transfer-lifecycle.sh. This executes the actual
// bridge implementation in an iOS Simulator process, without starting the app
// or registering its global media handlers. It tests the native entry point
// and stored state, not OS background time or a visible UIApplication window.
#ifndef NANO_NATIVE_BRIDGE_SOURCE
#define NANO_NATIVE_BRIDGE_SOURCE "../NativeBridge.m"
#endif
#include NANO_NATIVE_BRIDGE_SOURCE
#include <stdio.h>

static void check(BOOL condition, const char *message) {
    if (!condition) {
        fprintf(stderr, "FAIL: %s\n", message);
        exit(1);
    }
}

int main(void) {
    @autoreleasepool {
        check(NSThread.isMainThread, "native calls run on main thread");
        NMBridge *bridge = [NMBridge new];
        bridge.transferTask = UIBackgroundTaskInvalid;
        NMActiveBridge = bridge;
        atomic_store_explicit(&NMIsForeground, true, memory_order_release);
        atomic_store_explicit(&NMTransferAllowed, false, memory_order_release);
        void *handle = (__bridge void *)bridge;

        for (unsigned int cycle = 0; cycle < 100; ++cycle) {
            uint64_t before = nm_background_epoch();
            nm_bridge_set_transfer_active(handle, true);
            check(bridge.transferEnabled, "Start stores the enabled state without recursive calls");
            check(nm_transfer_allowed(), "Start opens the transfer gate");
            check(bridge.holdsIdleTimer, "Start owns the foreground idle override");
            nm_bridge_set_transfer_active(handle, true);
            check(nm_background_epoch() == before, "Repeated Start is idempotent");

            nm_bridge_set_transfer_active(handle, false);
            check(!bridge.transferEnabled, "Stop clears enabled state");
            check(!nm_transfer_allowed(), "Stop closes the transfer gate");
            check(!bridge.holdsIdleTimer, "Stop releases the idle override");
            check(nm_background_epoch() == before + 1, "Stop cancels the previous requests once");
            nm_bridge_set_transfer_active(handle, false);
            check(nm_background_epoch() == before + 1, "Repeated Stop is idempotent");
        }

        nm_bridge_set_transfer_active(handle, true);
        [bridge shutdown];
        check(bridge.closed && !bridge.transferEnabled && !bridge.holdsIdleTimer,
              "Shutdown releases an active transfer");
        nm_bridge_set_transfer_active(handle, true);
        check(!nm_transfer_allowed(), "Closed bridge cannot restart transfer");
        puts("PASS: 100 native Start/Stop cycles, repeated calls and shutdown");
    }
    return 0;
}

#pragma once

#include <stddef.h>
#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*NMEventCallback)(void *context, const char *json);
typedef void (*NMReleaseContext)(void *context);

// Context ownership transfers to native code, including on creation failure.
void *nm_bridge_create(void *context, NMEventCallback callback, NMReleaseContext release);
void nm_bridge_destroy(void *bridge);
// Returned error strings are owned; NULL means success.
char *nm_bridge_pick_music(void *bridge, const char *destination);
char *nm_bridge_prepare_playback(void *bridge, uint64_t *covered_epoch);
void nm_bridge_set_transfer_active(void *bridge, bool active);
// Borrowed arguments are copied before returning. NULL metadata clears it.
void nm_bridge_publish(void *bridge, const char *metadata, const uint8_t *artwork, size_t artwork_length);
// Returns an owned UTF-8 path, or NULL on failure.
char *nm_documents_dir(void);
// Write-only operation; returned error is owned, NULL means success.
char *nm_copy_text(const char *text);
// Read only after an explicit Paste action. Both returned text and *error are
// owned UTF-8 strings freed with nm_string_free. NULL text means failure.
char *nm_paste_text(char **error);
void nm_safe_area(float *top, float *right, float *bottom, float *left);
// Bottom occlusion in window points, including the bottom safe area.
float nm_keyboard_height(void);
// Safe from any thread; updated synchronously by UIKit lifecycle notifications.
bool nm_is_foreground(void);
bool nm_transfer_allowed(void);
uint64_t nm_background_epoch(void);
void nm_string_free(char *string);

#ifdef __cplusplus
}
#endif

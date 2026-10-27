// NSWorkspace wallpaper operations must run on the app's main thread.
#import <AppKit/AppKit.h>
#include <stdbool.h>
#include <string.h>

@interface NMWallpaper : NSObject
@property(nonatomic) NSURL *journal;
@property(nonatomic) NSMutableDictionary *originals;
@end
@implementation NMWallpaper
@end

static bool NMError(NSString *message, char *error, size_t capacity) {
    if (capacity) strlcpy(error, message.UTF8String, capacity);
    return false;
}

static NSString *NMScreenKey(NSScreen *screen) {
    return [screen.deviceDescription[@"NSScreenNumber"] stringValue];
}

static bool NMSave(NMWallpaper *state, char *error, size_t capacity) {
    NSError *failure = nil;
    NSData *data = [NSKeyedArchiver archivedDataWithRootObject:state.originals
                                     requiringSecureCoding:YES error:&failure];
    if (!data || ![data writeToURL:state.journal options:NSDataWritingAtomic error:&failure]) {
        return NMError(failure.localizedDescription ?: @"Cannot save original wallpaper", error, capacity);
    }
    return true;
}

static bool NMReferences(NMWallpaper *state, NSURL *url) {
    for (NSDictionary *entry in state.originals.allValues) {
        if ([url isEqual:entry[@"owned"]] || [url isEqual:entry[@"pending"]]) return true;
    }
    return false;
}

static void NMCleanup(NMWallpaper *state) {
    NSURL *directory = state.journal.URLByDeletingLastPathComponent;
    for (NSURL *url in [NSFileManager.defaultManager contentsOfDirectoryAtURL:directory
            includingPropertiesForKeys:nil options:0 error:NULL]) {
        if ([url.lastPathComponent hasPrefix:@"cover-"] && !NMReferences(state, url)) {
            [NSFileManager.defaultManager removeItemAtURL:url error:NULL];
        }
    }
}

// Restore only wallpapers still owned by this session. A manual system wallpaper
// change (or a different Space) must not be overwritten when the player exits.
static bool NMRestore(NMWallpaper *state, char *error, size_t capacity) {
    NSWorkspace *workspace = NSWorkspace.sharedWorkspace;
    bool success = true;
    NSMutableArray *restoredFiles = [NSMutableArray new];
    for (NSScreen *screen in NSScreen.screens) {
        NSString *key = NMScreenKey(screen);
        NSDictionary *entry = state.originals[key];
        if (!entry) continue;
        NSURL *current = [workspace desktopImageURLForScreen:screen];
        if ([current isEqual:entry[@"owned"]] || [current isEqual:entry[@"pending"]]) {
            NSError *failure = nil;
            if (![workspace setDesktopImageURL:entry[@"url"] forScreen:screen
                                       options:entry[@"options"] error:&failure]) {
                NMError(failure.localizedDescription ?: @"Cannot restore wallpaper", error, capacity);
                success = false;
                continue;
            }
            [restoredFiles addObject:entry[@"owned"]];
            if (entry[@"pending"]) [restoredFiles addObject:entry[@"pending"]];
        }
        [state.originals removeObjectForKey:key];
    }
    for (NSURL *url in restoredFiles) {
        if ([url.URLByDeletingLastPathComponent isEqual:state.journal.URLByDeletingLastPathComponent]
                && [url.lastPathComponent hasPrefix:@"cover-"] && !NMReferences(state, url)) {
            [NSFileManager.defaultManager removeItemAtURL:url error:NULL];
        }
    }
    // Retain disconnected displays for recovery when they reconnect.
    if (state.originals.count) {
        return NMSave(state, error, capacity) && success;
    }
    if ([NSFileManager.defaultManager fileExistsAtPath:state.journal.path]) {
        NSError *failure = nil;
        if (![NSFileManager.defaultManager removeItemAtURL:state.journal error:&failure]) {
            return NMError(failure.localizedDescription, error, capacity);
        }
    }
    return success;
}

void *nm_wallpaper_create(const char *journal, char *error, size_t capacity) {
    @autoreleasepool {
        if (!NSThread.isMainThread) {
            NMError(@"Wallpaper requires the main thread", error, capacity);
            return NULL;
        }
        NMWallpaper *state = [NMWallpaper new];
        state.journal = [NSURL fileURLWithPath:@(journal)];
        state.originals = [NSMutableDictionary new];
        if ([NSFileManager.defaultManager fileExistsAtPath:state.journal.path]) {
            NSError *failure = nil;
            NSData *data = [NSData dataWithContentsOfURL:state.journal options:0 error:&failure];
            NSSet *classes = [NSSet setWithObjects:NSDictionary.class, NSMutableDictionary.class,
                NSString.class, NSNumber.class, NSURL.class, NSColor.class, NSColorSpace.class, nil];
            NSDictionary *saved = data ? [NSKeyedUnarchiver unarchivedObjectOfClasses:classes
                                                                           fromData:data error:&failure] : nil;
            if (!saved) {
                NMError(failure.localizedDescription ?: @"Invalid wallpaper recovery file", error, capacity);
                return NULL;
            }
            state.originals = [saved mutableCopy];
        }
        return (__bridge_retained void *)state;
    }
}

bool nm_wallpaper_restore(void *handle, char *error, size_t capacity) {
    @autoreleasepool {
        if (!NSThread.isMainThread) return NMError(@"Wallpaper requires the main thread", error, capacity);
        return NMRestore((__bridge NMWallpaper *)handle, error, capacity);
    }
}

bool nm_wallpaper_apply(void *handle, const char *path, char *error, size_t capacity) {
    @autoreleasepool {
        if (!NSThread.isMainThread) return NMError(@"Wallpaper requires the main thread", error, capacity);
        NMWallpaper *state = (__bridge NMWallpaper *)handle;
        NSWorkspace *workspace = NSWorkspace.sharedWorkspace;
        NSURL *url = [NSURL fileURLWithPath:@(path)];
        if (!NSScreen.screens.count) return NMError(@"No desktop display available", error, capacity);
        for (NSScreen *screen in NSScreen.screens) {
            NSString *key = NMScreenKey(screen);
            NSURL *current = [workspace desktopImageURLForScreen:screen];
            NSDictionary *previous = state.originals[key];
            // Respect manual changes and do not follow the user into other Spaces.
            if (previous && ![current isEqual:previous[@"owned"]]) continue;
            if (!previous && !current) return NMError(@"Cannot read original wallpaper", error, capacity);
            NSDictionary *entry = @{
                @"url": previous ? previous[@"url"] : current,
                @"options": previous ? previous[@"options"] : ([workspace desktopImageOptionsForScreen:screen] ?: @{}),
                @"owned": url,
                @"pending": current
            };
            state.originals[key] = entry;
            // Persist before changing the system; the previous wallpaper survives a crash.
            if (!NMSave(state, error, capacity)) {
                if (previous) state.originals[key] = previous;
                else [state.originals removeObjectForKey:key];
                return false;
            }
            NSError *failure = nil;
            NSDictionary *options = @{NSWorkspaceDesktopImageScalingKey: @(NSImageScaleProportionallyUpOrDown),
                NSWorkspaceDesktopImageAllowClippingKey: @NO,
                NSWorkspaceDesktopImageFillColorKey: [NSColor colorWithWhite:0.055 alpha:1.0]};
            if (![workspace setDesktopImageURL:url forScreen:screen options:options error:&failure]) {
                if (previous) state.originals[key] = previous;
                else [state.originals removeObjectForKey:key];
                NMSave(state, error, capacity);
                return NMError(failure.localizedDescription ?: @"Cannot set wallpaper", error, capacity);
            }
        }
        NMCleanup(state);
        return true;
    }
}

void nm_wallpaper_destroy(void *handle) {
    @autoreleasepool {
        // Rust calls restore explicitly so failures can be reported to the user.
        NMWallpaper *state = CFBridgingRelease(handle);
        (void)state;
    }
}

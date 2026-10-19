// UIKit / media services only. Playback and application state remain in Rust.
#import "NativeBridge.h"

#import <AVFoundation/AVFoundation.h>
#import <Foundation/Foundation.h>
#import <MediaPlayer/MediaPlayer.h>
#import <UIKit/UIKit.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import <dispatch/dispatch.h>
#include <errno.h>
#include <math.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

static void NMOnMain(dispatch_block_t operation) {
    if ([NSThread isMainThread]) {
        operation();
    } else {
        dispatch_sync(dispatch_get_main_queue(), operation);
    }
}

static char *NMError(NSString *message) {
    return strdup((message ?: @"Unknown iOS service error").UTF8String);
}

// Winit owns the scene and window. Avoid UIApplication.keyWindow, which is
// ambiguous in scene-based applications, and never create a second UIWindow.
static UIWindow *NMWindow(void) {
    UIWindow *fallback = nil;
    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
        if (![scene isKindOfClass:UIWindowScene.class]) continue;
        if (![scene.session.role isEqualToString:UIWindowSceneSessionRoleApplication]) continue;
        if (scene.activationState != UISceneActivationStateForegroundActive &&
            scene.activationState != UISceneActivationStateForegroundInactive) continue;
        for (UIWindow *window in ((UIWindowScene *)scene).windows) {
            if (window.isKeyWindow) return window;
            if (!window.hidden && window.windowLevel == UIWindowLevelNormal) fallback = window;
        }
    }
    return fallback;
}

static UIViewController *NMPresenter(void) {
    UIViewController *controller = NMWindow().rootViewController;
    while (controller) {
        if (controller.presentedViewController && !controller.presentedViewController.isBeingDismissed) {
            controller = controller.presentedViewController;
        } else if ([controller isKindOfClass:UINavigationController.class]) {
            UIViewController *visible = ((UINavigationController *)controller).visibleViewController;
            if (!visible || visible == controller) break;
            controller = visible;
        } else if ([controller isKindOfClass:UITabBarController.class]) {
            UIViewController *selected = ((UITabBarController *)controller).selectedViewController;
            if (!selected || selected == controller) break;
            controller = selected;
        } else {
            break;
        }
    }
    return controller;
}

@interface NMBridge : NSObject <UIDocumentPickerDelegate, UIAdaptivePresentationControllerDelegate>
@property(nonatomic) void *context;
@property(nonatomic) NMEventCallback callback;
@property(nonatomic) NMReleaseContext releaseContext;
@property(nonatomic) BOOL closed;
@property(nonatomic) BOOL importing;
@property(nonatomic) BOOL holdsIdleTimer;
@property(nonatomic) BOOL previousIdleTimerDisabled;
@property(nonatomic) CGFloat keyboardHeight;
@property(nonatomic) uint64_t interruptionEpoch;
// Keep the stored state's setter distinct from setTransferActive:, which also
// manages UIKit resources. A matching property setter would call itself.
@property(nonatomic) BOOL transferEnabled;
@property(nonatomic) UIBackgroundTaskIdentifier transferTask;
@property(nonatomic) NSUInteger transferTaskGeneration;
@property(nonatomic, strong) UIDocumentPickerViewController *picker;
@property(nonatomic, strong) NSURL *destination;
@property(nonatomic, strong) NSMutableArray<NSArray *> *remoteTargets;
@property(nonatomic, strong) NSMutableArray<id> *observers;
@property(nonatomic, strong) NSData *artworkData;
@property(nonatomic, strong) MPMediaItemArtwork *artwork;
- (void)install;
- (void)shutdown;
- (void)emit:(NSDictionary *)event;
- (NSString *)pickMusic:(NSURL *)destination;
- (void)publish:(NSDictionary *)metadata artwork:(NSData *)artwork;
- (void)setTransferActive:(BOOL)active;
- (void)restoreIdleTimer;
- (void)updateKeyboard:(NSNotification *)notification;
- (void)beginTransferTask;
- (void)endTransferTask;
@end

// System media centers are shared. An obsolete bridge must not erase the
// current one's metadata or deliver duplicate remote commands.
static __weak NMBridge *NMActiveBridge;
static atomic_bool NMIsForeground = ATOMIC_VAR_INIT(true);
static atomic_bool NMTransferAllowed = ATOMIC_VAR_INIT(false);
static atomic_uint_fast64_t NMBackgroundEpoch = ATOMIC_VAR_INIT(0);

@implementation NMBridge

- (void)updateKeyboard:(NSNotification *)notification {
    NSAssert(NSThread.isMainThread, @"Keyboard layout must run on the main thread");
    if (self.closed || NMActiveBridge != self) return;
    self.keyboardHeight = 0;
    if ([notification.name isEqualToString:UIKeyboardWillHideNotification] ||
        [notification.name isEqualToString:UIKeyboardDidHideNotification] ||
        !atomic_load_explicit(&NMIsForeground, memory_order_acquire)) return;
    UIWindow *window = NMWindow();
    NSValue *frame = notification.userInfo[UIKeyboardFrameEndUserInfoKey];
    if (!window || ![frame isKindOfClass:NSValue.class]) return;
    // Since iOS 16.1 the notification names its screen. Earlier versions use
    // nil; fall back to the screen hosting our Slint window on iOS 15/16.0.
    UIScreen *screen = [notification.object isKindOfClass:UIScreen.class] ? notification.object : window.screen;
    if (screen != window.screen) return;
    CGRect converted = [window convertRect:frame.CGRectValue fromCoordinateSpace:screen.coordinateSpace];
    CGRect intersection = CGRectIntersection(window.bounds, converted);
    if (CGRectIsNull(intersection) || CGRectIsEmpty(intersection)) return;
    CGFloat height = CGRectGetMaxY(window.bounds) - CGRectGetMinY(intersection);
    if (isfinite(height)) self.keyboardHeight = MIN(CGRectGetHeight(window.bounds), MAX(0, height));
}

- (void)restoreIdleTimer {
    NSAssert(NSThread.isMainThread, @"Idle timer restoration must run on the main thread");
    if (!self.holdsIdleTimer) return;
    if (NMActiveBridge == self) {
        UIApplication.sharedApplication.idleTimerDisabled = self.previousIdleTimerDisabled;
    }
    self.holdsIdleTimer = NO;
}

- (void)setTransferActive:(BOOL)active {
    NSAssert(NSThread.isMainThread, @"Transfer activity must run on the main thread");
    if (self.closed || NMActiveBridge != self) return;
    if (!active) {
        BOOL wasActive = self.transferEnabled;
        self.transferEnabled = NO;
        atomic_store_explicit(&NMTransferAllowed, false, memory_order_release);
        if (wasActive) atomic_fetch_add_explicit(&NMBackgroundEpoch, 1, memory_order_release);
        [self endTransferTask];
        [self restoreIdleTimer];
        return;
    }
    self.transferEnabled = YES;
    if (!atomic_load_explicit(&NMIsForeground, memory_order_acquire)) {
        [self restoreIdleTimer];
        [self beginTransferTask];
        return;
    }
    atomic_store_explicit(&NMTransferAllowed, true, memory_order_release);
    if (!self.holdsIdleTimer) {
        UIApplication *application = UIApplication.sharedApplication;
        self.previousIdleTimerDisabled = application.idleTimerDisabled;
        application.idleTimerDisabled = YES;
        self.holdsIdleTimer = YES;
    }
}

- (void)endTransferTask {
    self.transferTaskGeneration += 1;
    UIBackgroundTaskIdentifier task = self.transferTask;
    self.transferTask = UIBackgroundTaskInvalid;
    if (task != UIBackgroundTaskInvalid) [UIApplication.sharedApplication endBackgroundTask:task];
}

- (void)beginTransferTask {
    if (!self.transferEnabled || self.closed || NMActiveBridge != self || self.transferTask != UIBackgroundTaskInvalid) return;
    self.transferTaskGeneration += 1;
    NSUInteger generation = self.transferTaskGeneration;
    __weak NMBridge *weakSelf = self;
    self.transferTask = [UIApplication.sharedApplication beginBackgroundTaskWithName:@"Finish music transfer"
        expirationHandler:^{
            NMOnMain(^{
                NMBridge *bridge = weakSelf;
                if (!bridge || bridge.closed || NMActiveBridge != bridge ||
                    bridge.transferTaskGeneration != generation || !bridge.transferEnabled) return;
                // Cancel only this interval's in-flight requests. The Rust
                // listener and pairing secret survive and resume in foreground.
                atomic_store_explicit(&NMTransferAllowed, false, memory_order_release);
                atomic_fetch_add_explicit(&NMBackgroundEpoch, 1, memory_order_release);
                [bridge endTransferTask];
                [bridge emit:@{@"type": @"transfer_paused"}];
            });
        }];
    BOOL allowed = self.transferTask != UIBackgroundTaskInvalid;
    atomic_store_explicit(&NMTransferAllowed, allowed, memory_order_release);
    if (!allowed) {
        atomic_fetch_add_explicit(&NMBackgroundEpoch, 1, memory_order_release);
        [self emit:@{@"type": @"transfer_paused"}];
    }
}

- (void)emit:(NSDictionary *)event {
    // Remote handlers may arrive off the main thread. Capturing self keeps
    // this object alive until shutdown has suppressed any pending callbacks.
    if (![NSThread isMainThread]) {
        dispatch_async(dispatch_get_main_queue(), ^{ [self emit:event]; });
        return;
    }
    if (self.closed || !self.callback || !self.context) return;
    NSData *data = [NSJSONSerialization dataWithJSONObject:event options:0 error:nil];
    NSString *json = data ? [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding] : nil;
    if (json) self.callback(self.context, json.UTF8String);
}

- (void)installCommand:(MPRemoteCommand *)command name:(NSString *)name {
    __weak NMBridge *weakSelf = self;
    id target = [command addTargetWithHandler:^MPRemoteCommandHandlerStatus(MPRemoteCommandEvent *event) {
        NMBridge *bridge = weakSelf;
        if (!bridge) return MPRemoteCommandHandlerStatusCommandFailed;
        NSMutableDictionary *payload = [@{@"type": @"remote", @"command": name} mutableCopy];
        if ([name isEqualToString:@"seek"]) {
            if (![event isKindOfClass:MPChangePlaybackPositionCommandEvent.class]) {
                return MPRemoteCommandHandlerStatusCommandFailed;
            }
            double position = ((MPChangePlaybackPositionCommandEvent *)event).positionTime;
            if (!isfinite(position)) return MPRemoteCommandHandlerStatusCommandFailed;
            payload[@"position"] = @(MAX(0.0, position));
        }
        // All bridge state (including the active route) is main-queue-owned.
        dispatch_async(dispatch_get_main_queue(), ^{
            if (NMActiveBridge == bridge && !bridge.closed) [bridge emit:payload];
        });
        return MPRemoteCommandHandlerStatusSuccess;
    }];
    if (target) [self.remoteTargets addObject:@[command, target]];
    command.enabled = YES;
}

- (void)install {
    NSAssert(NSThread.isMainThread, @"Native bridge installation must run on the main thread");
    self.remoteTargets = [NSMutableArray array];
    self.observers = [NSMutableArray array];
    // Release the old owner's override before a replacement can snapshot it.
    [NMActiveBridge setTransferActive:NO];
    self.transferTask = UIBackgroundTaskInvalid;
    NMActiveBridge = self;
    atomic_store_explicit(&NMTransferAllowed, false, memory_order_release);
    atomic_store_explicit(&NMIsForeground, UIApplication.sharedApplication.applicationState != UIApplicationStateBackground,
        memory_order_release);
    MPRemoteCommandCenter *center = MPRemoteCommandCenter.sharedCommandCenter;
    [self installCommand:center.playCommand name:@"play"];
    [self installCommand:center.pauseCommand name:@"pause"];
    [self installCommand:center.togglePlayPauseCommand name:@"toggle"];
    [self installCommand:center.nextTrackCommand name:@"next"];
    [self installCommand:center.previousTrackCommand name:@"previous"];
    [self installCommand:center.changePlaybackPositionCommand name:@"seek"];

    __weak NMBridge *weakSelf = self;
    NSNotificationCenter *notifications = NSNotificationCenter.defaultCenter;
    id interruption = [notifications addObserverForName:AVAudioSessionInterruptionNotification
        object:AVAudioSession.sharedInstance queue:NSOperationQueue.mainQueue
        usingBlock:^(NSNotification *notification) {
            NMBridge *bridge = weakSelf;
            if (!bridge || NMActiveBridge != bridge || bridge.closed) return;
            NSNumber *kind = notification.userInfo[AVAudioSessionInterruptionTypeKey];
            if (!kind) return;
            if (kind.unsignedIntegerValue == AVAudioSessionInterruptionTypeBegan) {
                bridge.interruptionEpoch += 1;
                [bridge emit:@{@"type": @"interruption_began", @"epoch": @(bridge.interruptionEpoch),
                    @"suspended": @([notification.userInfo[AVAudioSessionInterruptionWasSuspendedKey] boolValue]),
                    @"reason": notification.userInfo[AVAudioSessionInterruptionReasonKey] ?: @0}];
            } else if (kind.unsignedIntegerValue == AVAudioSessionInterruptionTypeEnded) {
                NSUInteger options = [notification.userInfo[AVAudioSessionInterruptionOptionKey] unsignedIntegerValue];
                [bridge emit:@{@"type": @"interruption_ended",
                    @"epoch": @(bridge.interruptionEpoch),
                    @"should_resume": @((options & AVAudioSessionInterruptionOptionShouldResume) != 0)}];
            }
        }];
    if (interruption) [self.observers addObject:interruption];
    id route = [notifications addObserverForName:AVAudioSessionRouteChangeNotification
        object:AVAudioSession.sharedInstance queue:NSOperationQueue.mainQueue
        usingBlock:^(NSNotification *notification) {
            NMBridge *bridge = weakSelf;
            if (!bridge || NMActiveBridge != bridge || bridge.closed) return;
            NSUInteger reason = [notification.userInfo[AVAudioSessionRouteChangeReasonKey] unsignedIntegerValue];
            if (reason == AVAudioSessionRouteChangeReasonOldDeviceUnavailable) {
                [bridge emit:@{@"type": @"output_disconnected"}];
            }
        }];
    if (route) [self.observers addObject:route];
    for (NSNotificationName name in @[UIKeyboardWillChangeFrameNotification, UIKeyboardDidChangeFrameNotification,
            UIKeyboardWillHideNotification, UIKeyboardDidHideNotification]) {
        id keyboard = [notifications addObserverForName:name object:nil queue:nil usingBlock:^(NSNotification *notification) {
            NMOnMain(^{ [weakSelf updateKeyboard:notification]; });
        }];
        if (keyboard) [self.observers addObject:keyboard];
    }
    // UIKit posts these on the main thread. queue:nil preserves synchronous
    // delivery: UIKit can grant a finite transfer allowance before Slint's
    // next timer tick, without changing the listener address or pairing secret.
    for (NSNotificationName name in @[UIApplicationWillResignActiveNotification, UIApplicationDidEnterBackgroundNotification]) {
        id lifecycle = [notifications addObserverForName:name object:nil queue:nil usingBlock:^(NSNotification *notification) {
            NMOnMain(^{
                NMBridge *bridge = weakSelf;
                if (!bridge || NMActiveBridge != bridge || bridge.closed) return;
                [bridge restoreIdleTimer];
                bridge.keyboardHeight = 0;
                atomic_store_explicit(&NMIsForeground, false, memory_order_release);
                if (bridge.transferEnabled) [bridge beginTransferTask];
                [bridge emit:@{@"type": @"entered_background"}];
            });
        }];
        if (lifecycle) [self.observers addObject:lifecycle];
    }
    id active = [notifications addObserverForName:UIApplicationDidBecomeActiveNotification object:nil queue:nil
        usingBlock:^(NSNotification *notification) {
            NMOnMain(^{
                NMBridge *bridge = weakSelf;
                if (bridge && NMActiveBridge == bridge && !bridge.closed) {
                    atomic_store_explicit(&NMIsForeground, true, memory_order_release);
                    [bridge endTransferTask];
                    if (bridge.transferEnabled) {
                        [bridge setTransferActive:YES];
                        [bridge emit:@{@"type": @"transfer_resumed"}];
                    }
                }
            });
        }];
    if (active) [self.observers addObject:active];
}

- (void)shutdown {
    NSAssert(NSThread.isMainThread, @"Native bridge shutdown must run on the main thread");
    if (self.closed) return;
    [self setTransferActive:NO];
    [self endTransferTask];
    self.closed = YES;
    self.keyboardHeight = 0;
    [self restoreIdleTimer];
    self.picker.delegate = nil;
    self.picker.presentationController.delegate = nil;
    if (self.picker.presentingViewController) [self.picker dismissViewControllerAnimated:NO completion:nil];
    self.picker = nil;
    self.destination = nil;
    for (NSArray *entry in self.remoteTargets) [(MPRemoteCommand *)entry[0] removeTarget:entry[1]];
    [self.remoteTargets removeAllObjects];
    for (id observer in self.observers) [NSNotificationCenter.defaultCenter removeObserver:observer];
    [self.observers removeAllObjects];
    if (NMActiveBridge == self) {
        NMActiveBridge = nil;
        atomic_store_explicit(&NMIsForeground, false, memory_order_release);
        MPNowPlayingInfoCenter.defaultCenter.nowPlayingInfo = nil;
        NSError *error = nil;
        [AVAudioSession.sharedInstance setActive:NO
            withOptions:AVAudioSessionSetActiveOptionNotifyOthersOnDeactivation error:&error];
    }
    self.artwork = nil;
    self.artworkData = nil;
    // No later task may call back into Rust after this release. Async import
    // tasks retain only self and their URLs; emit checks closed first.
    self.callback = NULL;
    if (self.releaseContext && self.context) self.releaseContext(self.context);
    self.context = NULL;
    self.releaseContext = NULL;
}

- (NSString *)pickMusic:(NSURL *)destination {
    if (self.closed) return @"iOS service bridge is closed";
    if (self.importing) return @"A music import is already in progress";
    UIViewController *presenter = NMPresenter();
    if (!presenter || !presenter.view.window || presenter.isBeingDismissed || presenter.isBeingPresented) {
        return @"Music picker is unavailable until the application window is visible";
    }
    if (!destination.isFileURL) return @"Music destination must be a local directory";
    UIDocumentPickerViewController *picker = [[UIDocumentPickerViewController alloc]
        initForOpeningContentTypes:@[UTTypeAudio] asCopy:NO];
    picker.allowsMultipleSelection = YES;
    picker.shouldShowFileExtensions = YES;
    picker.delegate = self;
    picker.modalPresentationStyle = UIModalPresentationFormSheet;
    self.picker = picker;
    self.destination = destination;
    self.importing = YES;
    [presenter presentViewController:picker animated:YES completion:nil];
    picker.presentationController.delegate = self;
    return nil;
}

- (void)finishImport:(NSUInteger)imported errors:(NSArray<NSString *> *)errors {
    self.picker.delegate = nil;
    self.picker.presentationController.delegate = nil;
    self.picker = nil;
    self.destination = nil;
    self.importing = NO;
    [self emit:@{@"type": @"import_finished", @"imported": @(imported), @"errors": errors}];
}

- (void)documentPickerWasCancelled:(UIDocumentPickerViewController *)controller {
    if (controller == self.picker) [self finishImport:0 errors:@[]];
}

- (void)presentationControllerDidDismiss:(UIPresentationController *)presentationController {
    if (self.picker && presentationController.presentedViewController == self.picker) {
        [self finishImport:0 errors:@[]];
    }
}

- (void)documentPicker:(UIDocumentPickerViewController *)controller didPickDocumentsAtURLs:(NSArray<NSURL *> *)urls {
    if (self.closed || controller != self.picker) return;
    NSURL *destination = self.destination;
    self.picker.delegate = nil;
    self.picker.presentationController.delegate = nil;
    self.picker = nil;
    // Acquire access inside the picker delegate and hold it across the async
    // coordinated read, including any iCloud/File Provider download latency.
    NSMutableArray<NSDictionary *> *resources = [NSMutableArray arrayWithCapacity:urls.count];
    for (NSURL *url in urls) {
        BOOL scoped = [url startAccessingSecurityScopedResource];
        [resources addObject:@{@"url": url, @"scoped": @(scoped)}];
    }
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        @autoreleasepool {
            NSFileManager *files = NSFileManager.defaultManager;
            NSMutableArray<NSString *> *errors = [NSMutableArray array];
            NSUInteger imported = 0;
            NSError *directoryError = nil;
            BOOL ready = destination && [files createDirectoryAtURL:destination
                withIntermediateDirectories:YES attributes:nil error:&directoryError];
            if (!ready) [errors addObject:directoryError.localizedDescription ?: @"Cannot create Music directory"];
            for (NSDictionary *resource in resources) {
                NSURL *source = resource[@"url"];
                @autoreleasepool {
                    if (ready) {
                        NSFileCoordinator *coordinator = [[NSFileCoordinator alloc] initWithFilePresenter:nil];
                        NSError *coordinationError = nil;
                        __block NSError *copyError = nil;
                        __block BOOL copied = NO;
                        [coordinator coordinateReadingItemAtURL:source options:0 error:&coordinationError
                            byAccessor:^(NSURL *readableURL) {
                                NSNumber *regular = nil;
                                if (![readableURL getResourceValue:&regular forKey:NSURLIsRegularFileKey error:&copyError]) return;
                                NSString *name = source.lastPathComponent;
                                // Keep this set aligned with library::paths::is_audio.
                                NSSet<NSString *> *supported = [NSSet setWithArray:@[@"mp3", @"flac", @"wav", @"m4a", @"aac", @"ogg", @"aiff"]];
                                if (!regular.boolValue || name.length == 0 || ![supported containsObject:name.pathExtension.lowercaseString]) {
                                    copyError = [NSError errorWithDomain:@"NanoMusicImport" code:1
                                        userInfo:@{NSLocalizedDescriptionKey: @"The selected document is not an audio file"}];
                                    return;
                                }
                                // A private UUID directory preserves the original extension and
                                // never replaces an existing song with the same file name.
                                NSURL *unique = [destination URLByAppendingPathComponent:NSUUID.UUID.UUIDString isDirectory:YES];
                                // mkdir claims the directory exclusively. Foundation treats
                                // an existing directory as success, which would make cleanup
                                // unsafe in the unlikely event of a UUID collision.
                                if (mkdir(unique.fileSystemRepresentation, 0700) != 0) {
                                    copyError = [NSError errorWithDomain:NSPOSIXErrorDomain code:errno userInfo:nil];
                                    return;
                                }
                                NSURL *temporary = [unique URLByAppendingPathComponent:@".importing"];
                                NSURL *target = [unique URLByAppendingPathComponent:name];
                                copied = [files copyItemAtURL:readableURL toURL:temporary error:&copyError] &&
                                    [files moveItemAtURL:temporary toURL:target error:&copyError];
                                if (!copied) [files removeItemAtURL:unique error:nil];
                            }];
                        if (copied) {
                            imported += 1;
                        } else {
                            NSError *error = copyError ?: coordinationError;
                            [errors addObject:[NSString stringWithFormat:@"%@: %@", source.lastPathComponent,
                                error.localizedDescription ?: @"Document could not be copied"]];
                        }
                    }
                    // A false return can also mean an already-readable sandbox
                    // URL. Attempt the read; balance only successful acquisitions.
                    if ([resource[@"scoped"] boolValue]) [source stopAccessingSecurityScopedResource];
                }
            }
            dispatch_async(dispatch_get_main_queue(), ^{ [self finishImport:imported errors:errors]; });
        }
    });
}

- (void)publish:(NSDictionary *)metadata artwork:(NSData *)artworkData {
    if (self.closed || NMActiveBridge != self) return;
    if (!metadata) {
        MPNowPlayingInfoCenter.defaultCenter.nowPlayingInfo = nil;
        self.artworkData = nil;
        self.artwork = nil;
        return;
    }
    NSMutableDictionary *info = [NSMutableDictionary dictionary];
    NSDictionary *mapping = @{@"title": MPMediaItemPropertyTitle,
        @"artist": MPMediaItemPropertyArtist, @"album": MPMediaItemPropertyAlbumTitle};
    for (NSString *key in mapping) {
        if ([metadata[key] isKindOfClass:NSString.class]) info[mapping[key]] = metadata[key];
    }
    NSNumber *duration = metadata[@"duration"];
    if ([duration isKindOfClass:NSNumber.class] && isfinite(duration.doubleValue) && duration.doubleValue > 0) {
        info[MPMediaItemPropertyPlaybackDuration] = duration;
    }
    double elapsed = [metadata[@"elapsed"] isKindOfClass:NSNumber.class] ? [metadata[@"elapsed"] doubleValue] : 0;
    info[MPNowPlayingInfoPropertyElapsedPlaybackTime] = @(isfinite(elapsed) ? MAX(0, elapsed) : 0);
    info[MPNowPlayingInfoPropertyPlaybackRate] = [metadata[@"state"] isEqual:@"playing"] ? @1.0 : @0.0;
    info[MPNowPlayingInfoPropertyDefaultPlaybackRate] = @1.0;
    info[MPNowPlayingInfoPropertyMediaType] = @(MPNowPlayingInfoMediaTypeAudio);
    info[MPNowPlayingInfoPropertyIsLiveStream] = @NO;
    if (![self.artworkData isEqualToData:artworkData]) {
        self.artworkData = artworkData;
        self.artwork = nil;
        UIImage *image = artworkData.length ? [UIImage imageWithData:artworkData] : nil;
        if (image && image.size.width > 0 && image.size.height > 0) {
            self.artwork = [[MPMediaItemArtwork alloc] initWithBoundsSize:image.size requestHandler:^UIImage *(CGSize requested) {
                if (requested.width <= 0 || requested.height <= 0) return image;
                CGFloat factor = MIN(1, MIN(requested.width / image.size.width, requested.height / image.size.height));
                CGSize size = CGSizeMake(MAX(1, image.size.width * factor), MAX(1, image.size.height * factor));
                UIGraphicsImageRendererFormat *format = UIGraphicsImageRendererFormat.defaultFormat;
                format.scale = 1;
                UIGraphicsImageRenderer *renderer = [[UIGraphicsImageRenderer alloc] initWithSize:size format:format];
                return [renderer imageWithActions:^(UIGraphicsImageRendererContext *context) {
                    [image drawInRect:(CGRect){CGPointZero, size}];
                }];
            }];
        }
    }
    if (self.artwork) info[MPMediaItemPropertyArtwork] = self.artwork;
    // playbackState belongs to macOS. iOS derives state from the playback
    // rate and AVAudioSession; setting it is neither needed nor portable.
    MPNowPlayingInfoCenter.defaultCenter.nowPlayingInfo = info;
}

@end

void *nm_bridge_create(void *context, NMEventCallback callback, NMReleaseContext release) {
    __block void *handle = NULL;
    NMOnMain(^{
        NMBridge *bridge = [[NMBridge alloc] init];
        if (!bridge) {
            if (release && context) release(context);
            return;
        }
        bridge.context = context;
        bridge.callback = callback;
        bridge.releaseContext = release;
        [bridge install];
        handle = (__bridge_retained void *)bridge;
    });
    return handle;
}

void nm_bridge_destroy(void *handle) {
    if (!handle) return;
    NMOnMain(^{
        NMBridge *bridge = (__bridge_transfer NMBridge *)handle;
        [bridge shutdown];
    });
}

char *nm_bridge_pick_music(void *handle, const char *path) {
    if (!handle || !path) return NMError(@"Music picker received an invalid bridge or directory");
    NSString *destination = [[NSString alloc] initWithUTF8String:path];
    if (!destination.length) return NMError(@"Music directory is not valid UTF-8");
    __block NSString *error = nil;
    NMOnMain(^{ error = [(__bridge NMBridge *)handle pickMusic:[NSURL fileURLWithPath:destination isDirectory:YES]]; });
    return error ? NMError(error) : NULL;
}

char *nm_bridge_prepare_playback(void *handle, uint64_t *covered_epoch) {
    if (!handle) return NMError(@"iOS service bridge is unavailable");
    __block NSError *error = nil;
    __block BOOL ok = NO;
    NMOnMain(^{
        NMBridge *bridge = (__bridge NMBridge *)handle;
        if (bridge.closed) return;
        // Capture before activation: a fresh interruption raised during or
        // after setActive must not be discarded as an already handled event.
        if (covered_epoch) *covered_epoch = bridge.interruptionEpoch;
        AVAudioSession *session = AVAudioSession.sharedInstance;
        ok = [session setCategory:AVAudioSessionCategoryPlayback mode:AVAudioSessionModeDefault options:0 error:&error] &&
            [session setActive:YES error:&error];
    });
    return ok ? NULL : NMError(error.localizedDescription ?: @"Cannot activate audio playback session");
}

void nm_bridge_set_transfer_active(void *handle, bool active) {
    if (!handle) return;
    NMOnMain(^{ [(__bridge NMBridge *)handle setTransferActive:active]; });
}

void nm_bridge_publish(void *handle, const char *json, const uint8_t *artwork, size_t artworkLength) {
    if (!handle) return;
    NMBridge *bridge = (__bridge NMBridge *)handle;
    NSDictionary *metadata = nil;
    if (json) {
        NSData *data = [NSData dataWithBytes:json length:strlen(json)];
        id value = [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
        if (![value isKindOfClass:NSDictionary.class]) return;
        metadata = value;
    }
    NSData *image = artwork && artworkLength ? [NSData dataWithBytes:artwork length:artworkLength] : nil;
    // Copy every borrowed Rust value above before this asynchronous handoff.
    dispatch_async(dispatch_get_main_queue(), ^{ [bridge publish:metadata artwork:image]; });
}

char *nm_documents_dir(void) {
    @autoreleasepool {
        NSURL *directory = [NSFileManager.defaultManager URLsForDirectory:NSDocumentDirectory inDomains:NSUserDomainMask].firstObject;
        return directory ? strdup(directory.path.UTF8String) : NULL;
    }
}

char *nm_copy_text(const char *text) {
    if (!text) return NMError(@"Clipboard text is missing");
    @autoreleasepool {
        NSString *value = [[NSString alloc] initWithUTF8String:text];
        if (!value) return NMError(@"Clipboard text is not valid UTF-8");
        __block NSString *error = nil;
        NMOnMain(^{
            UIPasteboard *pasteboard = UIPasteboard.generalPasteboard;
            if (pasteboard) {
                // Setting a string never reads existing clipboard contents.
                pasteboard.string = value;
            } else {
                error = @"The system clipboard is unavailable";
            }
        });
        return error ? NMError(error) : NULL;
    }
}

char *nm_paste_text(char **error) {
    if (error) *error = NULL;
    @autoreleasepool {
        __block NSString *value = nil;
        __block NSString *failure = nil;
        NMOnMain(^{
            UIPasteboard *pasteboard = UIPasteboard.generalPasteboard;
            if (!pasteboard) {
                failure = @"The system clipboard is unavailable";
                return;
            }
            // This function is called only by the user's explicit Paste action.
            // UIKit owns any system paste-access prompt; no startup/poll reads.
            value = pasteboard.string;
            if (!value) failure = @"The clipboard has no text or paste access was denied";
        });
        const char *utf8 = value.UTF8String;
        if (!failure && (!utf8 || strlen(utf8) != [value lengthOfBytesUsingEncoding:NSUTF8StringEncoding])) {
            failure = @"Clipboard text cannot be pasted as UTF-8";
        }
        char *text = failure ? NULL : strdup(utf8);
        if (!failure && !text) failure = @"Clipboard text could not be copied";
        if (failure && error) *error = NMError(failure);
        return text;
    }
}

void nm_safe_area(float *top, float *right, float *bottom, float *left) {
    __block UIEdgeInsets insets = UIEdgeInsetsZero;
    NMOnMain(^{
        UIWindow *window = NMWindow();
        if (window) insets = window.safeAreaInsets;
    });
    if (top) *top = (float)insets.top;
    if (right) *right = (float)insets.right;
    if (bottom) *bottom = (float)insets.bottom;
    if (left) *left = (float)insets.left;
}

float nm_keyboard_height(void) {
    __block float height = 0;
    NMOnMain(^{
        NMBridge *bridge = NMActiveBridge;
        if (bridge && !bridge.closed) height = (float)bridge.keyboardHeight;
    });
    return height;
}

void nm_string_free(char *string) {
    free(string);
}

bool nm_is_foreground(void) {
    return atomic_load_explicit(&NMIsForeground, memory_order_acquire);
}

bool nm_transfer_allowed(void) {
    return atomic_load_explicit(&NMTransferAllowed, memory_order_acquire);
}

uint64_t nm_background_epoch(void) {
    return (uint64_t)atomic_load_explicit(&NMBackgroundEpoch, memory_order_acquire);
}

// Deterministic NSWorkspace tests: no real desktop or wallpaper is modified.
#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#include "../src/platform/wallpaper.m"

@interface NMTestScreen : NSObject
@property(nonatomic) NSDictionary *deviceDescription;
@end
@implementation NMTestScreen
@end

@interface NMTestWorkspace : NSObject
@property(nonatomic) NSMutableDictionary *urls;
@property(nonatomic) NSMutableDictionary *options;
@property(nonatomic) NSString *failScreen;
@property(nonatomic) NSUInteger changes;
@end
@implementation NMTestWorkspace
- (NSURL *)desktopImageURLForScreen:(NSScreen *)screen { return self.urls[NMScreenKey(screen)]; }
- (NSDictionary *)desktopImageOptionsForScreen:(NSScreen *)screen { return self.options[NMScreenKey(screen)]; }
- (BOOL)setDesktopImageURL:(NSURL *)url forScreen:(NSScreen *)screen options:(NSDictionary *)options error:(NSError **)error {
    NSString *key = NMScreenKey(screen);
    if ([key isEqual:self.failScreen]) {
        if (error) *error = [NSError errorWithDomain:@"test" code:1 userInfo:@{NSLocalizedDescriptionKey: @"Simulated failure"}];
        return NO;
    }
    self.urls[key] = url;
    self.options[key] = options;
    self.changes++;
    return YES;
}
@end

static NMTestWorkspace *workspace;
static NSArray *screens;
static id Workspace(id self, SEL cmd) { return workspace; }
static id Screens(id self, SEL cmd) { return screens; }

int main(void) {
    @autoreleasepool {
        method_setImplementation(class_getClassMethod(NSWorkspace.class, @selector(sharedWorkspace)), (IMP)Workspace);
        method_setImplementation(class_getClassMethod(NSScreen.class, @selector(screens)), (IMP)Screens);
        workspace = [NMTestWorkspace new];
        NMTestScreen *first = [NMTestScreen new], *second = [NMTestScreen new];
        first.deviceDescription = @{@"NSScreenNumber": @1};
        second.deviceDescription = @{@"NSScreenNumber": @2};
        screens = @[first, second];
        NSURL *original = [NSURL fileURLWithPath:@"/original.jpg"];
        NSDictionary *originalOptions = @{NSWorkspaceDesktopImageScalingKey: @(NSImageScaleAxesIndependently),
            NSWorkspaceDesktopImageFillColorKey: [NSColor colorWithSRGBRed:0.2 green:0.3 blue:0.4 alpha:1.0]};
        workspace.urls = [@{@"1": original, @"2": original} mutableCopy];
        workspace.options = [@{@"1": originalOptions, @"2": originalOptions} mutableCopy];
        NSString *directory = [NSTemporaryDirectory() stringByAppendingPathComponent:NSUUID.UUID.UUIDString];
        NSFileManager *files = NSFileManager.defaultManager;
        [files createDirectoryAtPath:directory withIntermediateDirectories:YES attributes:nil error:NULL];
        NSString *journal = [directory stringByAppendingPathComponent:@"originals.archive"];
        NSString *cover = [directory stringByAppendingPathComponent:@"cover-test.png"];
        [@"fixture" writeToFile:cover atomically:YES encoding:NSUTF8StringEncoding error:NULL];
        char error[1024] = {0};
        void *state = nm_wallpaper_create(journal.UTF8String, error, sizeof(error));
        assert(state);
        assert(nm_wallpaper_apply(state, cover.UTF8String, error, sizeof(error)));
        assert(workspace.changes == 2);
        assert([[workspace.urls[@"1"] path] isEqual:cover]);
        assert([files fileExistsAtPath:journal]);
        nm_wallpaper_destroy(state); // Simulate an interrupted app without restore.
        state = nm_wallpaper_create(journal.UTF8String, error, sizeof(error));
        assert(state); // Exercise secure archive decoding, including NSColor.
        assert(nm_wallpaper_restore(state, error, sizeof(error)));
        assert([workspace.urls[@"1"] isEqual:original]);
        assert([workspace.options[@"1"] isEqual:originalOptions]);
        assert(![files fileExistsAtPath:journal]);
        assert(![files fileExistsAtPath:cover]);

        [@"fixture" writeToFile:cover atomically:YES encoding:NSUTF8StringEncoding error:NULL];
        assert(nm_wallpaper_apply(state, cover.UTF8String, error, sizeof(error)));
        NSURL *manual = [NSURL fileURLWithPath:@"/manual.jpg"];
        workspace.urls[@"1"] = manual;
        NSUInteger before = workspace.changes;
        assert(nm_wallpaper_apply(state, cover.UTF8String, error, sizeof(error)));
        assert(workspace.changes == before + 1); // Manual change on screen 1 is respected.
        assert(nm_wallpaper_restore(state, error, sizeof(error)));
        assert([workspace.urls[@"1"] isEqual:manual]);
        assert([workspace.urls[@"2"] isEqual:original]);

        // A partial multi-display failure still restores displays already changed.
        [@"fixture" writeToFile:cover atomically:YES encoding:NSUTF8StringEncoding error:NULL];
        workspace.failScreen = @"2";
        assert(!nm_wallpaper_apply(state, cover.UTF8String, error, sizeof(error)));
        assert(strstr(error, "Simulated failure"));
        workspace.failScreen = nil;
        assert(nm_wallpaper_restore(state, error, sizeof(error)));
        assert([workspace.urls[@"1"] isEqual:manual]);
        assert([workspace.urls[@"2"] isEqual:original]);
        nm_wallpaper_destroy(state);
        [files removeItemAtPath:directory error:NULL];
        puts("Native wallpaper tests passed: recovery, options, manual changes, partial failure.");
    }
    return 0;
}

// Desktop-only, read-only system services. No Bluetooth connection or microphone access.
#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#import <IOKit/ps/IOPowerSources.h>
#import <IOKit/ps/IOPSKeys.h>
#include <stdatomic.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>

static AudioObjectPropertyAddress NMAddress(AudioObjectPropertySelector selector,
                                           AudioObjectPropertyScope scope) {
    return (AudioObjectPropertyAddress){selector, scope, kAudioObjectPropertyElementMain};
}

static UInt32 NMNumber(AudioObjectID object, AudioObjectPropertySelector selector,
                       AudioObjectPropertyScope scope) {
    AudioObjectPropertyAddress address = NMAddress(selector, scope);
    UInt32 result = 0, size = sizeof(result);
    if (AudioObjectGetPropertyData(object, &address, 0, NULL, &size, &result) != noErr) return 0;
    return result;
}

static NSString *NMString(AudioObjectID object, AudioObjectPropertySelector selector) {
    AudioObjectPropertyAddress address = NMAddress(selector, kAudioObjectPropertyScopeGlobal);
    CFStringRef result = NULL;
    UInt32 size = sizeof(result);
    if (AudioObjectGetPropertyData(object, &address, 0, NULL, &size, &result) != noErr) return @"";
    return CFBridgingRelease(result) ?: @"";
}

static NSDictionary *NMOutput(void) {
    AudioDeviceID device = NMNumber(kAudioObjectSystemObject, kAudioHardwarePropertyDefaultOutputDevice,
                                   kAudioObjectPropertyScopeGlobal);
    if (!device) return @{};
    UInt32 transport = NMNumber(device, kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal);
    BOOL bluetooth = transport == kAudioDeviceTransportTypeBluetooth || transport == kAudioDeviceTransportTypeBluetoothLE;
    BOOL headphones = bluetooth;
    // Wired headphones may share a built-in device with the speakers.
    headphones |= NMNumber(device, kAudioDevicePropertyDataSource, kAudioDevicePropertyScopeOutput) == 'hdpn';
    AudioObjectPropertyAddress address = NMAddress(kAudioDevicePropertyStreams, kAudioDevicePropertyScopeOutput);
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(device, &address, 0, NULL, &size) == noErr && size > 0) {
        AudioStreamID *streams = malloc(size);
        if (streams) {
            if (AudioObjectGetPropertyData(device, &address, 0, NULL, &size, streams) == noErr) {
                for (UInt32 i = 0; i < size / sizeof(AudioStreamID); i++) {
                    headphones |= NMNumber(streams[i], kAudioStreamPropertyTerminalType,
                                           kAudioObjectPropertyScopeGlobal) == kAudioStreamTerminalTypeHeadphones;
                }
            }
            free(streams);
        }
    }
    return @{@"uid": NMString(device, kAudioDevicePropertyDeviceUID),
             @"name": NMString(device, kAudioObjectPropertyName),
             @"bluetooth": @(bluetooth), @"headphones": @(headphones)};
}

// Owned JSON buffer; Rust frees with libc::free. Missing capacity stays unknown.
char *nm_desktop_status_copy(void) {
    @autoreleasepool {
        NSMutableDictionary *result = [@{@"output": NMOutput()} mutableCopy];
        CFTypeRef info = IOPSCopyPowerSourcesInfo();
        if (info) {
            NSArray *sources = CFBridgingRelease(IOPSCopyPowerSourcesList(info));
            for (id source in sources) {
                NSDictionary *description = (__bridge NSDictionary *)IOPSGetPowerSourceDescription(info, (__bridge CFTypeRef)source);
                if (![description[@kIOPSTypeKey] isEqual:@kIOPSInternalBatteryType] ||
                    ![description[@kIOPSIsPresentKey] boolValue]) continue;
                NSNumber *current = description[@kIOPSCurrentCapacityKey], *maximum = description[@kIOPSMaxCapacityKey];
                if (current && maximum && current.doubleValue >= 0 && maximum.doubleValue > 0) {
                    result[@"battery_percent"] = @(MIN(100, MAX(0, lround(100 * current.doubleValue / maximum.doubleValue))));
                }
                result[@"charging"] = @([description[@kIOPSIsChargingKey] boolValue]);
                result[@"external_power"] = @([description[@kIOPSPowerSourceStateKey] isEqual:@kIOPSACPowerValue]);
                break;
            }
            CFRelease(info);
        }
        NSData *data = [NSJSONSerialization dataWithJSONObject:result options:0 error:NULL];
        return data ? strndup(data.bytes, data.length) : NULL;
    }
}

@interface NMOutputWatch : NSObject {
@public
    atomic_uint events;
}
@property(nonatomic, strong) dispatch_queue_t queue;
@property(nonatomic, copy) AudioObjectPropertyListenerBlock listener;
@property(nonatomic) BOOL headphones;
@property(nonatomic) BOOL installed;
@end

@implementation NMOutputWatch
@end

void *nm_desktop_watch_create(void) {
    @autoreleasepool {
        NMOutputWatch *watch = [NMOutputWatch new];
        atomic_init(&watch->events, 0);
        watch.headphones = [NMOutput()[@"headphones"] boolValue];
        watch.queue = dispatch_queue_create("org.nano.output-watch", DISPATCH_QUEUE_SERIAL);
        __weak NMOutputWatch *weakWatch = watch;
        watch.listener = ^(UInt32 count, const AudioObjectPropertyAddress *addresses) {
            (void)count;
            (void)addresses;
            @autoreleasepool {
                NMOutputWatch *current = weakWatch;
                if (!current) return;
                BOOL headphones = [NMOutput()[@"headphones"] boolValue];
                // Pause on a private-output -> speaker transition, including a manual switch.
                unsigned flags = 1 | ((current.headphones && !headphones) ? 2 : 0);
                current.headphones = headphones;
                atomic_fetch_or_explicit(&current->events, flags, memory_order_release);
            }
        };
        AudioObjectPropertyAddress address = NMAddress(kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyScopeGlobal);
        watch.installed = AudioObjectAddPropertyListenerBlock(kAudioObjectSystemObject, &address, watch.queue, watch.listener) == noErr;
        return (__bridge_retained void *)watch;
    }
}

unsigned nm_desktop_watch_take_events(void *handle) {
    NMOutputWatch *watch = (__bridge NMOutputWatch *)handle;
    return atomic_exchange_explicit(&watch->events, 0, memory_order_acq_rel);
}

void nm_desktop_watch_destroy(void *handle) {
    @autoreleasepool {
        NMOutputWatch *watch = CFBridgingRelease(handle);
        if (watch.installed) {
            AudioObjectPropertyAddress address = NMAddress(kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyScopeGlobal);
            AudioObjectRemovePropertyListenerBlock(kAudioObjectSystemObject, &address, watch.queue, watch.listener);
        }
        // Run after any in-flight callbacks before dropping the final owner.
        dispatch_sync(watch.queue, ^{});
    }
}

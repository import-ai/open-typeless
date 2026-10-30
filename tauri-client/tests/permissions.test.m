#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <CoreBluetooth/CoreBluetooth.h>
#import <ApplicationServices/ApplicationServices.h>
#import <IOKit/hidsystem/IOHIDLib.h>
#include <assert.h>

static AVAuthorizationStatus microphoneStatus;
static CBManagerAuthorization bluetoothStatus;
static BOOL trusted;
static unsigned int microphoneRequests, accessibilityRequests, bluetoothRequests, inputRequests;
static void (^microphoneDecision)(BOOL);
static NSString *openedURL;

@interface TestCaptureDevice : NSObject
+ (AVAuthorizationStatus)authorizationStatusForMediaType:(AVMediaType)type;
+ (void)requestAccessForMediaType:(AVMediaType)type completionHandler:(void (^)(BOOL))handler;
@end
@implementation TestCaptureDevice
+ (AVAuthorizationStatus)authorizationStatusForMediaType:(AVMediaType)type { (void)type; return microphoneStatus; }
+ (void)requestAccessForMediaType:(AVMediaType)type completionHandler:(void (^)(BOOL))handler {
    (void)type; microphoneRequests++; microphoneDecision = handler;
}
@end

@interface TestBluetoothManager : NSObject
+ (CBManagerAuthorization)authorization;
@end
@implementation TestBluetoothManager
+ (CBManagerAuthorization)authorization { return bluetoothStatus; }
@end

@interface TestCentralManager : NSObject
- (instancetype)initWithDelegate:(id)delegate queue:(dispatch_queue_t)queue;
@end
@implementation TestCentralManager
- (instancetype)initWithDelegate:(id)delegate queue:(dispatch_queue_t)queue {
    (void)delegate; (void)queue; self = [super init]; if (self) bluetoothRequests++; return self;
}
@end

@interface TestWorkspace : NSObject
+ (instancetype)sharedWorkspace;
- (BOOL)openURL:(NSURL *)url;
@end
@implementation TestWorkspace
+ (instancetype)sharedWorkspace { return [self new]; }
- (BOOL)openURL:(NSURL *)url { openedURL = url.absoluteString; return YES; }
@end

static Boolean testTrusted(void) { return trusted; }
static Boolean testRequestTrust(CFDictionaryRef options) {
    assert(CFDictionaryGetValue(options, kAXTrustedCheckOptionPrompt) == kCFBooleanTrue);
    accessibilityRequests++;
    return trusted;
}
static IOHIDAccessType testCheckInput(IOHIDRequestType type) { (void)type; return kIOHIDAccessTypeDenied; }
static bool testRequestInput(IOHIDRequestType type) { assert(type == kIOHIDRequestTypeListenEvent); inputRequests++; return false; }

#define AVCaptureDevice TestCaptureDevice
#define CBManager TestBluetoothManager
#define CBCentralManager TestCentralManager
#define NSWorkspace TestWorkspace
#define AXIsProcessTrusted testTrusted
#define AXIsProcessTrustedWithOptions testRequestTrust
#define IOHIDCheckAccess testCheckInput
#define IOHIDRequestAccess testRequestInput
#import "../src/permissions.m"

int main(void) {
    @autoreleasepool {
        microphoneStatus = AVAuthorizationStatusNotDetermined;
        bluetoothStatus = CBManagerAuthorizationNotDetermined;
        for (unsigned int permission = 0; permission < 4; permission++) (void)ot_permission_status(permission);
        assert(microphoneRequests == 0 && accessibilityRequests == 0 && bluetoothRequests == 0 && inputRequests == 0);
        assert(openedURL == nil);

        ot_request_startup_permissions();
        assert(microphoneRequests == 1 && accessibilityRequests == 0);
        microphoneDecision(NO);
        [[NSRunLoop mainRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]];
        assert(accessibilityRequests == 1);
        assert(bluetoothRequests == 0 && inputRequests == 0);

        microphoneStatus = AVAuthorizationStatusDenied;
        ot_request_startup_permissions();
        assert(microphoneRequests == 1 && accessibilityRequests == 2);
        trusted = YES;
        ot_request_startup_permissions();
        assert(accessibilityRequests == 2);

        assert(ot_request_permission(0));
        assert([openedURL hasSuffix:@"Privacy_Microphone"] && microphoneRequests == 1);
        trusted = NO;
        assert(ot_request_permission(1));
        assert([openedURL hasSuffix:@"Privacy_Accessibility"]);
        assert(ot_request_permission(2) && bluetoothRequests == 1);
        bluetoothStatus = CBManagerAuthorizationDenied;
        assert(ot_request_permission(2));
        assert([openedURL hasSuffix:@"Privacy_Bluetooth"] && bluetoothRequests == 1);
        assert(ot_request_permission(3) && inputRequests == 1);
        assert([openedURL hasSuffix:@"Privacy_ListenEvent"]);
        openedURL = nil;
        assert(!ot_request_permission(99) && ot_permission_status(99) == 5 && openedURL == nil);
        puts("PASS: read-only status, sequential startup prompts, denied permissions, settings routing, and invalid requests");
    }
}

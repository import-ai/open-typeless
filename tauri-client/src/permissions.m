#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <CoreBluetooth/CoreBluetooth.h>
#import <ApplicationServices/ApplicationServices.h>
#import <IOKit/hidsystem/IOHIDLib.h>

// Status codes match AVAuthorizationStatus and CBManagerAuthorization.
int ot_permission_status(unsigned int permission) {
    switch (permission) {
        case 0:
            if (@available(macOS 10.14, *)) return (int)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
            return 5;
        case 1: return AXIsProcessTrusted() ? 3 : 4;
        case 2:
            if (@available(macOS 10.15, *)) return (int)[CBManager authorization];
            return 5;
        case 3:
            if (@available(macOS 10.15, *)) {
                switch (IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)) {
                    case kIOHIDAccessTypeGranted: return 3;
                    case kIOHIDAccessTypeDenied: return 2;
                    default: return 0;
                }
            }
            return 5;
        default: return 5;
    }
}

static void requestAccessibility(void) {
    if (!AXIsProcessTrusted()) {
        AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)@{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES});
    }
}

void ot_request_startup_permissions(void) {
    if (@available(macOS 10.14, *)) {
        if (ot_permission_status(0) == 0) {
            // Wait for the microphone decision before showing the next prompt.
            [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) {
                (void)granted;
                dispatch_async(dispatch_get_main_queue(), ^{ requestAccessibility(); });
            }];
            return;
        }
    }
    requestAccessibility();
}

// Called on the main thread, only after the user presses an authorization button.
bool ot_request_permission(unsigned int permission) {
    static CBCentralManager *bluetooth;
    switch (permission) {
        case 0:
            if (@available(macOS 10.14, *)) {
                if (ot_permission_status(0) == 0) {
                    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) { (void)granted; }];
                    return true;
                }
            } else { return false; }
            break;
        case 1:
            if (AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)@{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES})) return true;
            break;
        case 2:
            if (@available(macOS 10.15, *)) {
                if (ot_permission_status(2) == 0) {
                    bluetooth = [[CBCentralManager alloc] initWithDelegate:nil queue:dispatch_get_main_queue()];
                    return bluetooth != nil;
                }
            } else { return false; }
            break;
        case 3:
            if (@available(macOS 10.15, *)) {
                if (IOHIDRequestAccess(kIOHIDRequestTypeListenEvent)) return true;
            } else { return false; }
            break;
        default: return false;
    }
    NSString *panes[] = {@"Microphone", @"Accessibility", @"Bluetooth", @"ListenEvent"};
    NSURL *url = [NSURL URLWithString:[@"x-apple.systempreferences:com.apple.preference.security?Privacy_" stringByAppendingString:panes[permission]]];
    return [[NSWorkspace sharedWorkspace] openURL:url];
}

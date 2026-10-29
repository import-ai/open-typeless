#import <CoreBluetooth/CoreBluetooth.h>
#import <Foundation/Foundation.h>
#import <IOKit/hid/IOHIDManager.h>
#import <IOKit/hidsystem/IOHIDLib.h>

// Events: status, voice button, ATVV control, ADPCM audio, transport failure.
typedef void (*RemoteCallback)(unsigned int, const unsigned char *, size_t);
static RemoteCallback callback;
static NSString *const serviceID = @"AB5E0001-5A21-4F05-BC7D-AF01F617B664";
static NSString *const txID = @"AB5E0002-5A21-4F05-BC7D-AF01F617B664";
static NSString *const audioID = @"AB5E0003-5A21-4F05-BC7D-AF01F617B664";
static NSString *const controlID = @"AB5E0004-5A21-4F05-BC7D-AF01F617B664";

static void emit(unsigned int kind, NSData *data) {
    if (callback) callback(kind, data.bytes, data.length);
}
static void status(NSString *message) { emit(1, [message dataUsingEncoding:NSUTF8StringEncoding]); }

@interface OTRemoteMicrophone : NSObject <CBCentralManagerDelegate, CBPeripheralDelegate>
@property CBCentralManager *central;
@property CBPeripheral *peripheral;
@property CBCharacteristic *tx;
@property CBCharacteristic *audio;
@property CBCharacteristic *control;
@property NSTimer *watchdog;
@property NSDate *deadline;
@property NSDate *keepAlive;
@property IOHIDManagerRef hid;
@property BOOL enabled;
@property BOOL held;
@property BOOL negotiated;
@property BOOL requested;
@property BOOL opening;
@property BOOL streaming;
@property BOOL closing;
@property uint8_t streamID;
- (void)button:(BOOL)down;
- (void)write:(NSData *)data;
- (void)closeMicrophone;
- (void)findRemote;
- (void)fail:(NSString *)message;
@end

static void hidValue(void *context, IOReturn result, void *sender, IOHIDValueRef value) {
    (void)sender;
    if (result != kIOReturnSuccess) return;
    IOHIDElementRef element = IOHIDValueGetElement(value);
    // Ignore duplicate array elements; this remote exposes a separate F5 element.
    if (IOHIDElementGetUsagePage(element) == 7 && IOHIDElementGetUsage(element) == 0x3e) {
        [(__bridge OTRemoteMicrophone *)context button:IOHIDValueGetIntegerValue(value) != 0];
    }
}
static void hidRemoved(void *context, IOReturn result, void *sender, IOHIDDeviceRef device) {
    (void)result; (void)sender; (void)device;
    OTRemoteMicrophone *remote = (__bridge OTRemoteMicrophone *)context;
    [remote button:NO];
    [remote fail:@"遥控器已断开，请重新连接"];
}

@implementation OTRemoteMicrophone
- (void)setListening:(BOOL)enabled {
    if (!enabled) {
        [self closeMicrophone];
        self.enabled = NO;
        self.held = NO;
        self.opening = NO;
        self.streaming = NO;
        self.closing = NO;
        self.deadline = nil;
        [self.watchdog invalidate];
        self.watchdog = nil;
        [self.central stopScan];
        if (self.peripheral) [self.central cancelPeripheralConnection:self.peripheral];
        self.peripheral = nil;
        self.central = nil;
        if (self.hid) {
            IOHIDManagerUnscheduleFromRunLoop(self.hid, CFRunLoopGetMain(), kCFRunLoopDefaultMode);
            IOHIDManagerClose(self.hid, kIOHIDOptionsTypeNone);
            CFRelease(self.hid);
            self.hid = NULL;
        }
        return;
    }
    self.enabled = YES;
    status(@"正在连接小米遥控器…");
    if (@available(macOS 10.15, *)) {
        if (IOHIDCheckAccess(kIOHIDRequestTypeListenEvent) != kIOHIDAccessTypeGranted) {
            IOHIDRequestAccess(kIOHIDRequestTypeListenEvent);
            [self fail:@"请在系统设置 → 隐私与安全性 → 输入监控中允许 Open Typeless，然后重启应用"];
            return;
        }
    } else {
        [self fail:@"遥控器麦克风需要 macOS 10.15 或更新版本"];
        return;
    }
    self.hid = IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDOptionsTypeNone);
    IOHIDManagerSetDeviceMatching(self.hid, (__bridge CFDictionaryRef)@{@"VendorID": @0x2717, @"ProductID": @0x32b8});
    IOHIDManagerRegisterInputValueCallback(self.hid, hidValue, (__bridge void *)self);
    IOHIDManagerRegisterDeviceRemovalCallback(self.hid, hidRemoved, (__bridge void *)self);
    IOHIDManagerScheduleWithRunLoop(self.hid, CFRunLoopGetMain(), kCFRunLoopDefaultMode);
    if (IOHIDManagerOpen(self.hid, kIOHIDOptionsTypeNone) != kIOReturnSuccess) {
        [self fail:@"无法监听语音键，请在系统设置 → 隐私与安全性 → 输入监控中允许 Open Typeless，然后重启应用"];
        return;
    }
    self.central = [[CBCentralManager alloc] initWithDelegate:self queue:dispatch_get_main_queue()];
    __weak OTRemoteMicrophone *weakSelf = self;
    self.watchdog = [NSTimer scheduledTimerWithTimeInterval:1 repeats:YES block:^(NSTimer *timer) {
        (void)timer;
        OTRemoteMicrophone *remote = weakSelf;
        if (!remote.enabled || remote.central.state != CBManagerStatePoweredOn) return;
        if (remote.deadline && remote.deadline.timeIntervalSinceNow < 0) {
            [remote fail:@"遥控器响应超时，正在重新连接"];
            remote.deadline = nil;
        } else if (!remote.peripheral) {
            [remote findRemote];
        } else if (remote.streaming && !remote.deadline && remote.keepAlive.timeIntervalSinceNow < 0) {
            const uint8_t keepAlive[] = {0x0e, remote.streamID};
            [remote write:[NSData dataWithBytes:keepAlive length:sizeof(keepAlive)]];
            remote.keepAlive = [NSDate dateWithTimeIntervalSinceNow:10];
        }
    }];
}
- (void)button:(BOOL)down {
    if (!self.enabled || self.held == down) return;
    fprintf(stderr, "remote voice key: %s\n", down ? "pressed" : "released");
    self.held = down;
    uint8_t value = down;
    emit(2, [NSData dataWithBytes:&value length:1]);
    if (!down) [self closeMicrophone];
}
- (void)fail:(NSString *)message {
    self.negotiated = NO;
    self.opening = NO;
    self.streaming = NO;
    self.closing = NO;
    self.held = NO;
    emit(5, [message dataUsingEncoding:NSUTF8StringEncoding]);
    if (self.peripheral) [self.central cancelPeripheralConnection:self.peripheral];
}
- (void)centralManagerDidUpdateState:(CBCentralManager *)central {
    if (central != self.central || !self.enabled) return;
    if (central.state == CBManagerStatePoweredOn) [self findRemote];
    else if (central.state == CBManagerStateUnauthorized) [self fail:@"请在系统设置 → 隐私与安全性 → 蓝牙中允许 Open Typeless"];
    else if (central.state == CBManagerStatePoweredOff) {
        [self fail:@"蓝牙已关闭，请开启蓝牙"];
        self.peripheral = nil;
    }
    else if (central.state == CBManagerStateUnsupported) [self fail:@"此 Mac 不支持蓝牙遥控器麦克风"];
}
- (BOOL)matches:(CBPeripheral *)peripheral {
    return [peripheral.name isEqualToString:@"小米蓝牙语音遥控器"];
}
- (void)findRemote {
    if (!self.enabled || self.peripheral) return;
    NSArray *devices = [self.central retrieveConnectedPeripheralsWithServices:@[
        [CBUUID UUIDWithString:@"1812"], [CBUUID UUIDWithString:serviceID]]];
    NSMutableArray *matching = [NSMutableArray array];
    for (CBPeripheral *device in devices) if ([self matches:device]) [matching addObject:device];
    if (matching.count > 1) { [self fail:@"检测到多个小米遥控器，请只连接一个后重试"]; return; }
    if (matching.count == 1) { [self connect:matching.firstObject]; return; }
    [self.central scanForPeripheralsWithServices:nil options:nil];
}
- (void)connect:(CBPeripheral *)peripheral {
    self.peripheral = peripheral;
    self.negotiated = NO;
    self.requested = NO;
    self.opening = NO;
    self.streaming = NO;
    self.closing = NO;
    self.tx = nil;
    self.audio = nil;
    self.control = nil;
    self.deadline = [NSDate dateWithTimeIntervalSinceNow:12];
    [self.central stopScan];
    [self.central connectPeripheral:peripheral options:nil];
}
- (void)centralManager:(CBCentralManager *)central didDiscoverPeripheral:(CBPeripheral *)peripheral advertisementData:(NSDictionary *)data RSSI:(NSNumber *)RSSI {
    if (central == self.central && self.enabled && !self.peripheral && [self matches:peripheral]) [self connect:peripheral];
}
- (void)centralManager:(CBCentralManager *)central didConnectPeripheral:(CBPeripheral *)peripheral {
    if (!self.enabled || peripheral != self.peripheral) return;
    peripheral.delegate = self;
    [peripheral discoverServices:@[[CBUUID UUIDWithString:serviceID]]];
}
- (void)disconnected:(CBPeripheral *)peripheral {
    if (peripheral != self.peripheral) return;
    self.peripheral = nil;
    self.negotiated = NO;
    self.opening = NO;
    self.streaming = NO;
    self.closing = NO;
    self.held = NO;
    self.deadline = nil;
    if (self.enabled) [self fail:@"遥控器已断开，正在等待重新连接"];
}
- (void)centralManager:(CBCentralManager *)central didFailToConnectPeripheral:(CBPeripheral *)peripheral error:(NSError *)error { if (central == self.central) [self disconnected:peripheral]; }
- (void)centralManager:(CBCentralManager *)central didDisconnectPeripheral:(CBPeripheral *)peripheral error:(NSError *)error { if (central == self.central) [self disconnected:peripheral]; }
- (void)peripheral:(CBPeripheral *)peripheral didDiscoverServices:(NSError *)error {
    if (peripheral != self.peripheral) return;
    if (error || peripheral.services.count != 1) { [self fail:@"遥控器未提供 ATVV 语音服务"]; return; }
    [peripheral discoverCharacteristics:@[[CBUUID UUIDWithString:txID], [CBUUID UUIDWithString:audioID], [CBUUID UUIDWithString:controlID]] forService:peripheral.services.firstObject];
}
- (void)peripheral:(CBPeripheral *)peripheral didDiscoverCharacteristicsForService:(CBService *)service error:(NSError *)error {
    if (peripheral != self.peripheral) return;
    if (error) { [self fail:@"无法读取遥控器语音特征"]; return; }
    for (CBCharacteristic *item in service.characteristics) {
        NSString *uuid = item.UUID.UUIDString.uppercaseString;
        if ([uuid isEqualToString:txID]) self.tx = item;
        if ([uuid isEqualToString:audioID]) self.audio = item;
        if ([uuid isEqualToString:controlID]) self.control = item;
    }
    if (!self.tx || !self.audio || !self.control || !(self.tx.properties & (CBCharacteristicPropertyWrite | CBCharacteristicPropertyWriteWithoutResponse))) {
        [self fail:@"遥控器语音特征不完整"]; return;
    }
    [peripheral setNotifyValue:YES forCharacteristic:self.audio];
    [peripheral setNotifyValue:YES forCharacteristic:self.control];
}
- (void)peripheral:(CBPeripheral *)peripheral didUpdateNotificationStateForCharacteristic:(CBCharacteristic *)item error:(NSError *)error {
    if (peripheral != self.peripheral) return;
    if (error) { [self fail:@"无法订阅遥控器语音通知"]; return; }
    if (self.audio.isNotifying && self.control.isNotifying && !self.requested) {
        self.requested = YES;
        const uint8_t caps[] = {0x0a, 1, 0, 0, 3, 3};
        [self write:[NSData dataWithBytes:caps length:sizeof(caps)]];
    }
}
- (void)write:(NSData *)data {
    if (!self.enabled || !self.tx || self.peripheral.state != CBPeripheralStateConnected) return;
    CBCharacteristicWriteType type = self.tx.properties & CBCharacteristicPropertyWriteWithoutResponse ? CBCharacteristicWriteWithoutResponse : CBCharacteristicWriteWithResponse;
    if (type == CBCharacteristicWriteWithoutResponse && !self.peripheral.canSendWriteWithoutResponse) {
        [self fail:@"遥控器蓝牙写入队列已满"]; return;
    }
    [self.peripheral writeValue:data forCharacteristic:self.tx type:type];
}
- (void)peripheral:(CBPeripheral *)peripheral didWriteValueForCharacteristic:(CBCharacteristic *)item error:(NSError *)error {
    if (peripheral == self.peripheral && error) [self fail:@"遥控器拒绝语音控制命令"];
}
- (void)openMicrophone {
    fprintf(stderr, "remote open: enabled=%d held=%d negotiated=%d opening=%d streaming=%d closing=%d\n", self.enabled, self.held, self.negotiated, self.opening, self.streaming, self.closing);
    // AUDIO_START can precede HID and the backend health check; adopt that stream.
    if (!self.enabled || !self.held || !self.negotiated || self.opening || self.streaming || self.closing) return;
    self.opening = YES;
    self.deadline = [NSDate dateWithTimeIntervalSinceNow:3];
    const uint8_t open[] = {0x0c, 0};
    [self write:[NSData dataWithBytes:open length:sizeof(open)]];
}
- (void)closeMicrophone {
    if (self.closing || (!self.opening && !self.streaming)) return;
    self.closing = YES;
    fprintf(stderr, "remote close: stream=%u\n", self.streamID);
    const uint8_t close[] = {0x0d, self.streamID};
    [self write:[NSData dataWithBytes:close length:sizeof(close)]];
    self.opening = NO;
    // Keep accepting the queued audio tail until AUDIO_STOP.
    self.deadline = [NSDate dateWithTimeIntervalSinceNow:2];
}
- (void)peripheral:(CBPeripheral *)peripheral didUpdateValueForCharacteristic:(CBCharacteristic *)item error:(NSError *)error {
    if (peripheral != self.peripheral || !self.enabled) return;
    if (error) { [self fail:@"遥控器音频传输失败"]; return; }
    NSData *data = item.value;
    if (!data.length) return;
    if (item == self.control) {
        const uint8_t *bytes = data.bytes;
        if (bytes[0] == 0 || bytes[0] == 4 || bytes[0] == 8) fprintf(stderr, "remote control: 0x%02x\n", bytes[0]);
        if (bytes[0] == 0x0b) { self.negotiated = YES; self.deadline = nil; }
        if (bytes[0] == 4 && data.length >= 4) {
            self.streamID = bytes[3];
            self.streaming = YES;
            self.deadline = nil;
            self.keepAlive = [NSDate dateWithTimeIntervalSinceNow:10];
            if (self.closing) {
                // A quick release can precede AUDIO_START and its actual stream ID.
                self.closing = NO;
                [self closeMicrophone];
            }
        }
        if (bytes[0] == 0) { self.streaming = NO; self.opening = NO; self.closing = NO; self.deadline = nil; }
        emit(3, data);
    } else if (item == self.audio && self.streaming) {
        emit(4, data);
    }
}
@end

static OTRemoteMicrophone *remote;
void ot_remote_enable(RemoteCallback handler, bool enabled) {
    dispatch_async(dispatch_get_main_queue(), ^{
        callback = handler;
        if (!remote) remote = [OTRemoteMicrophone new];
        if (remote.enabled != enabled) [remote setListening:enabled];
    });
}
void ot_remote_open(void) { dispatch_async(dispatch_get_main_queue(), ^{ [remote openMicrophone]; }); }
void ot_remote_close(void) { dispatch_async(dispatch_get_main_queue(), ^{ [remote closeMicrophone]; }); }
void ot_remote_reset(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
        if (remote.peripheral) [remote.central cancelPeripheralConnection:remote.peripheral];
    });
}

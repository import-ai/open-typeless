#import "../src/remote_microphone.m"
#include <assert.h>

@interface TestRemote : OTRemoteMicrophone
@property NSMutableArray<NSData *> *writes;
@property NSMutableArray<NSString *> *errors;
@end
@implementation TestRemote
- (void)write:(NSData *)data { [self.writes addObject:data]; }
- (void)fail:(NSString *)message { [self.errors addObject:message]; }
@end

static void control(TestRemote *remote, NSData *data) {
    ((CBMutableCharacteristic *)remote.control).value = data;
    [remote peripheral:remote.peripheral didUpdateValueForCharacteristic:remote.control error:nil];
}

int main(void) {
    @autoreleasepool {
        TestRemote *remote = [TestRemote new];
        remote.writes = [NSMutableArray array];
        remote.errors = [NSMutableArray array];
        remote.enabled = YES;
        remote.negotiated = YES;
        remote.peripheral = (CBPeripheral *)[NSObject new];
        remote.control = [[CBMutableCharacteristic alloc] initWithType:[CBUUID UUIDWithString:controlID]
            properties:CBCharacteristicPropertyNotify value:nil permissions:CBAttributePermissionsReadable];
        const uint8_t start[] = {4, 0, 2, 9};
        const uint8_t stop[] = {0, 0};
        const uint8_t search[] = {8};
        const uint8_t open[] = {0x0c, 0};
        const uint8_t close[] = {0x0d, 9};
        NSData *started = [NSData dataWithBytes:start length:sizeof(start)];
        NSData *stopped = [NSData dataWithBytes:stop length:sizeof(stop)];
        NSData *searched = [NSData dataWithBytes:search length:sizeof(search)];

        // Firmware may start audio without START_SEARCH; no HID event is required.
        control(remote, started);
        control(remote, searched);
        [remote openMicrophone];
        assert(remote.streaming && remote.errors.count == 0 && remote.writes.count == 0);
        control(remote, stopped);
        assert(!remote.streaming && !remote.opening && !remote.closing);
        assert(remote.writes.count == 0);

        control(remote, searched);
        control(remote, searched);
        assert(remote.opening && remote.writes.count == 1);
        assert([remote.writes[0] isEqual:[NSData dataWithBytes:open length:sizeof(open)]]);
        control(remote, started);
        control(remote, searched);
        assert(remote.writes.count == 1 && remote.streaming);
        [remote closeMicrophone];
        [remote closeMicrophone];
        assert(remote.writes.count == 2);
        assert([remote.writes[1] isEqual:[NSData dataWithBytes:close length:sizeof(close)]]);
        control(remote, stopped);
        assert(remote.errors.count == 0 && !remote.streaming && !remote.closing);
        puts("PASS: ATVV alone starts/stops audio; duplicate requests and cancellation are idempotent");
    }
}

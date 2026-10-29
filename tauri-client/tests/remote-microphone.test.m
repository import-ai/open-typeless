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
        ((CBMutableCharacteristic *)remote.control).value = [NSData dataWithBytes:start length:sizeof(start)];
        [remote peripheral:remote.peripheral didUpdateValueForCharacteristic:remote.control error:nil];
        [remote button:YES];
        [remote openMicrophone];
        assert(remote.streaming && remote.held);
        assert(remote.errors.count == 0 && remote.writes.count == 0);

        [remote button:NO];
        [remote closeMicrophone];
        const uint8_t close[] = {0x0d, 9};
        assert(remote.writes.count == 1);
        assert([remote.writes[0] isEqual:[NSData dataWithBytes:close length:sizeof(close)]]);
        [remote openMicrophone];
        assert(remote.errors.count == 0 && remote.writes.count == 1);
        const uint8_t stop[] = {0, 0};
        ((CBMutableCharacteristic *)remote.control).value = [NSData dataWithBytes:stop length:sizeof(stop)];
        [remote peripheral:remote.peripheral didUpdateValueForCharacteristic:remote.control error:nil];
        [remote button:YES];
        [remote openMicrophone];
        [remote openMicrophone];
        const uint8_t open[] = {0x0c, 0};
        assert(remote.writes.count == 2 && remote.errors.count == 0);
        assert([remote.writes[1] isEqual:[NSData dataWithBytes:open length:sizeof(open)]]);
        puts("PASS: both event orders work; open/close are idempotent without disconnecting");
    }
}

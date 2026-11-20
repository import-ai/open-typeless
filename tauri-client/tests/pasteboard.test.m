#include <assert.h>
#import "../src/pasteboard.m"

int main(void) {
    @autoreleasepool {
        NSPasteboard *pasteboard = [NSPasteboard pasteboardWithUniqueName];
        [pasteboard clearContents];
        NSData *image = [[NSImage imageWithSize:NSMakeSize(1, 1) flipped:NO drawingHandler:^BOOL(NSRect rect) {
            [NSColor.redColor setFill];
            NSRectFill(rect);
            return YES;
        }] TIFFRepresentation];
        assert(image);
        NSPasteboardItem *first = [NSPasteboardItem new];
        assert([first setData:image forType:NSPasteboardTypeTIFF]);
        assert([first setString:@"Image caption" forType:NSPasteboardTypeString]);
        NSPasteboardItem *second = [NSPasteboardItem new];
        assert([second setString:@"file:///tmp/example" forType:NSPasteboardTypeFileURL]);
        assert(([pasteboard writeObjects:@[first, second]]));

        assert(writeTemporaryText(pasteboard, @"Dictation"));
        assert([[pasteboard stringForType:NSPasteboardTypeString] isEqualToString:@"Dictation"]);
        assert(!writeTemporaryText(pasteboard, @"Overlapping dictation"));
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 400 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
            NSArray<NSPasteboardItem *> *items = pasteboard.pasteboardItems;
            assert(items.count == 2);
            assert([[items[0] dataForType:NSPasteboardTypeTIFF] isEqualToData:image]);
            assert([[items[0] stringForType:NSPasteboardTypeString] isEqualToString:@"Image caption"]);
            assert([[items[1] stringForType:NSPasteboardTypeFileURL] isEqualToString:@"file:///tmp/example"]);

            assert(writeTemporaryText(pasteboard, @"Second dictation"));
            [pasteboard clearContents];
            assert([pasteboard setString:@"New copy" forType:NSPasteboardTypeString]);
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 400 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
                assert([[pasteboard stringForType:NSPasteboardTypeString] isEqualToString:@"New copy"]);
                [pasteboard clearContents];
                assert(writeTemporaryText(pasteboard, @"Empty clipboard"));
                dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 400 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
                    assert(pasteboard.pasteboardItems.count == 0);
                    [pasteboard releaseGlobally];
                    exit(0);
                });
            });
        });
        dispatch_main();
    }
}

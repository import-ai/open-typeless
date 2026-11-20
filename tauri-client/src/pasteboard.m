#import <AppKit/AppKit.h>

static void restorePasteboard(NSPasteboard *pasteboard, NSInteger change, NSArray<NSPasteboardItem *> *items) {
    if (pasteboard.changeCount != change) return;
    [pasteboard clearContents];
    if (items.count && ![pasteboard writeObjects:items]) {
        NSLog(@"Open Typeless could not restore the clipboard after automatic paste");
    }
}

static BOOL writeTemporaryText(NSPasteboard *pasteboard, NSString *text) {
    // All callers run on the main thread. Do not snapshot our own temporary text.
    static BOOL pending;
    if (pending || !text) return NO;

    NSInteger originalChange = pasteboard.changeCount;
    NSArray<NSPasteboardItem *> *original = pasteboard.pasteboardItems;
    if (!original) return NO;
    NSMutableArray<NSPasteboardItem *> *snapshot = [NSMutableArray arrayWithCapacity:original.count];
    for (NSPasteboardItem *item in original) {
        NSPasteboardItem *copy = [NSPasteboardItem new];
        for (NSPasteboardType type in item.types) {
            NSData *data = [item dataForType:type];
            if (!data || ![copy setData:data forType:type]) return NO;
        }
        if (!copy.types.count) return NO;
        [snapshot addObject:copy];
    }

    if (pasteboard.changeCount != originalChange) return NO;
    NSInteger temporaryChange = [pasteboard clearContents];
    if (![pasteboard setString:text forType:NSPasteboardTypeString]) {
        restorePasteboard(pasteboard, temporaryChange, snapshot);
        return NO;
    }
    pending = YES;
    // shortcut: Paste has no read acknowledgement, so allow 250 ms for the target to read.
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 250 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
        restorePasteboard(pasteboard, temporaryChange, snapshot);
        pending = NO;
    });
    return YES;
}

bool ot_write_temporary_paste_text(const char *bytes, size_t length) {
    NSString *text = [[NSString alloc] initWithBytes:bytes length:length encoding:NSUTF8StringEncoding];
    return writeTemporaryText(NSPasteboard.generalPasteboard, text);
}

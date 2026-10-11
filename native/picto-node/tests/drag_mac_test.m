// macOS: clang -fobjc-arc -framework Cocoa native/picto-node/tests/drag_mac_test.m
//        -o /tmp/picto-drag-test && /tmp/picto-drag-test
#import "../src/drag_mac.m"

@interface LateDragView : NSView
@property BOOL inspectedWindow;
@end
@implementation LateDragView
- (NSWindow *)window {
    self.inspectedWindow = YES;
    return nil;
}
@end

int main(void) {
    @autoreleasepool {
        for (NSUInteger state = 0; state < 8; state++) {
            NSCAssert(picto_drag_source_ready(state & 1, (state & 2) != 0, (state & 4) != 0)
                == (state == 7), @"Drag requires left button, active application and focused window");
        }
        NSCAssert(([NSEvent pressedMouseButtons] & 1) == 0, @"Run with the mouse released");
        LateDragView *view = [[LateDragView alloc] init];
        view.inspectedWindow = NO;
        const char *paths[] = {"/tmp/late-drag.png"};
        picto_start_file_drag((__bridge void *)view, paths, 1, NULL, 0, 0);
        NSCAssert(!view.inspectedWindow, @"A late mouse-up request must never start an OS drag");
        NSArray<NSURL *> *urls = @[
            [NSURL fileURLWithPath:@"/tmp/one file.png"],
            [NSURL fileURLWithPath:@"/tmp/two.png"],
            [NSURL fileURLWithPath:@"/tmp/three.png"],
            [NSURL fileURLWithPath:@"/tmp/four.png"]
        ];
        NSImage *icon = [[NSImage alloc] initWithSize:NSMakeSize(64, 64)];
        NSArray<NSDraggingItem *> *items = picto_file_drag_items(urls, icon, NSZeroPoint);
        NSCAssert(items.count == urls.count, @"Each file must have its own dragging item");
        NSPasteboard *pasteboard = [NSPasteboard pasteboardWithUniqueName];
        NSMutableArray *writers = [NSMutableArray array];
        for (NSUInteger i = 0; i < items.count; i++) {
            [writers addObject:items[i].item];
            NSCAssert(items[i].draggingFrame.size.width == (i == 0 ? 64 : 1), @"Only one composite preview");
        }
        NSCAssert([pasteboard writeObjects:writers], @"File URL writers must be accepted");
        NSArray *read = [pasteboard readObjectsForClasses:@[[NSURL class]]
            options:@{NSPasteboardURLReadingFileURLsOnlyKey: @YES}];
        NSCAssert([read isEqualToArray:urls], @"All files must round-trip through the OS pasteboard");
        [pasteboard releaseGlobally];
        NSLog(@"PASS: focus/button guards; late drag rejected; four file URLs round-trip, one visible composite icon");
    }
    return 0;
}

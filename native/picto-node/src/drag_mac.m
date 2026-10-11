#import <Cocoa/Cocoa.h>

// ── Drag source ──────────────────────────────────────────────────────────

@interface PictoDragSource : NSObject <NSDraggingSource>
@end

@implementation PictoDragSource

- (NSDragOperation)draggingSession:(NSDraggingSession *)session
    sourceOperationMaskForDraggingContext:(NSDraggingContext)context {
    return NSDragOperationCopy;
}

@end

// Each physical file needs its own pasteboard writer. Only the first item
// carries the composite preview; transparent followers avoid stacked icons.
static NSArray<NSDraggingItem *> *picto_file_drag_items(
    NSArray<NSURL *> *urls, NSImage *icon, NSPoint position
) {
    NSMutableArray<NSDraggingItem *> *items = [NSMutableArray arrayWithCapacity:urls.count];
    NSImage *transparent = [[NSImage alloc] initWithSize:NSMakeSize(1, 1)];
    for (NSURL *url in urls) {
        NSDraggingItem *item = [[NSDraggingItem alloc] initWithPasteboardWriter:url];
        NSImage *image = items.count == 0 ? icon : transparent;
        NSSize size = image.size;
        [item setDraggingFrame:NSMakeRect(position.x - size.width / 2,
                                         position.y - size.height / 2,
                                         size.width, size.height) contents:image];
        [items addObject:item];
    }
    return items;
}

// ── Public C entry point ─────────────────────────────────────────────────

static BOOL picto_drag_source_ready(NSUInteger buttons, BOOL appActive, BOOL keyWindow) {
    return (buttons & 1) != 0 && appActive && keyWindow;
}

void picto_start_file_drag(
    void *ns_view_ptr,
    const char **paths,
    int path_count,
    const uint8_t *rgba_data,
    int icon_width,
    int icon_height
) {
    if (!ns_view_ptr || path_count <= 0 || !paths) return;
    // IPC can arrive after mouse-up. Starting an OS drag then has no matching
    // release event and can leave a floating drag image blocking other apps.
    if (([NSEvent pressedMouseButtons] & 1) == 0) return;

    NSView *view = (__bridge NSView *)ns_view_ptr;
    // Never steal input from another app or another Picto window.
    if (!picto_drag_source_ready([NSEvent pressedMouseButtons], NSApp.isActive, view.window.isKeyWindow)) return;

    // ── Build composite icon from RGBA buffer ──────────────────────────
    NSImage *compositeIcon = nil;
    if (rgba_data && icon_width > 0 && icon_height > 0) {
        NSBitmapImageRep *rep = [[NSBitmapImageRep alloc]
            initWithBitmapDataPlanes:NULL
            pixelsWide:icon_width
            pixelsHigh:icon_height
            bitsPerSample:8
            samplesPerPixel:4
            hasAlpha:YES
            isPlanar:NO
            colorSpaceName:NSDeviceRGBColorSpace
            bytesPerRow:icon_width * 4
            bitsPerPixel:32];
        memcpy([rep bitmapData], rgba_data, icon_width * icon_height * 4);
        compositeIcon = [[NSImage alloc] initWithSize:NSMakeSize(icon_width, icon_height)];
        [compositeIcon addRepresentation:rep];
    }

    if (!compositeIcon) {
        compositeIcon = [[NSImage alloc] initWithSize:NSMakeSize(64, 64)];
    }

    // ── Collect file URLs ──────────────────────────────────────────────
    NSMutableArray<NSURL *> *fileURLs = [NSMutableArray arrayWithCapacity:path_count];
    for (int i = 0; i < path_count; i++) {
        [fileURLs addObject:[NSURL fileURLWithPath:[NSString stringWithUTF8String:paths[i]]]];
    }

    NSPoint windowPos = [view.window mouseLocationOutsideOfEventStream];
    NSPoint viewPos = [view convertPoint:windowPos fromView:nil];
    NSArray<NSDraggingItem *> *dragItems = picto_file_drag_items(fileURLs, compositeIcon, viewPos);

    // ── Synthetic mouse event ──────────────────────────────────────────
    NSEvent *dragEvent = [NSEvent
        mouseEventWithType:NSEventTypeLeftMouseDragged
        location:windowPos
        modifierFlags:0
        timestamp:[[NSProcessInfo processInfo] systemUptime]
        windowNumber:view.window.windowNumber
        context:nil
        eventNumber:0
        clickCount:1
        pressure:1.0];

    // ── Begin drag session with one writer per file ────────────────────────────────
    PictoDragSource *source = [[PictoDragSource alloc] init];
    NSDraggingSession *session = [view beginDraggingSessionWithItems:dragItems
                                                              event:dragEvent
                                                             source:source];
    session.animatesToStartingPositionsOnCancelOrFail = YES;
}

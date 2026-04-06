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

// ── Public C entry point ─────────────────────────────────────────────────

void picto_start_file_drag(
    void *ns_view_ptr,
    const char **paths,
    int path_count,
    const uint8_t *rgba_data,
    int icon_width,
    int icon_height
) {
    if (!ns_view_ptr || path_count <= 0 || !paths) return;

    NSView *view = (__bridge NSView *)ns_view_ptr;
    if (!view.window) return;

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

    // ── Create ONE pasteboard item with all file URLs ──────────────────
    // Using NSPasteboardItem so there's only ONE NSDraggingItem = ONE icon.
    NSPasteboardItem *pbItem = [[NSPasteboardItem alloc] init];

    // Write file URLs as a newline-separated string for NSPasteboardTypeFileURL
    NSMutableString *urlList = [NSMutableString string];
    for (NSURL *url in fileURLs) {
        if (urlList.length > 0) [urlList appendString:@"\n"];
        [urlList appendString:url.absoluteString];
    }
    [pbItem setString:urlList forType:NSPasteboardTypeFileURL];

    // Also write as legacy NSFilenamesPboardType for older apps
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    NSMutableArray<NSString *> *pathStrings = [NSMutableArray arrayWithCapacity:path_count];
    for (int i = 0; i < path_count; i++) {
        [pathStrings addObject:[NSString stringWithUTF8String:paths[i]]];
    }
    [pbItem setPropertyList:pathStrings forType:NSFilenamesPboardType];
#pragma clang diagnostic pop

    // ── Create ONE dragging item with our composite icon ───────────────
    NSDraggingItem *dragItem = [[NSDraggingItem alloc] initWithPasteboardWriter:pbItem];

    NSPoint windowPos = [view.window mouseLocationOutsideOfEventStream];
    NSPoint viewPos = [view convertPoint:windowPos fromView:nil];
    NSSize imgSize = compositeIcon.size;
    NSRect dragFrame = NSMakeRect(
        viewPos.x - imgSize.width / 2,
        viewPos.y - imgSize.height / 2,
        imgSize.width,
        imgSize.height);

    [dragItem setDraggingFrame:dragFrame contents:compositeIcon];

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

    // ── Begin drag session with ONE item ────────────────────────────────
    PictoDragSource *source = [[PictoDragSource alloc] init];
    NSDraggingSession *session = [view beginDraggingSessionWithItems:@[dragItem]
                                                              event:dragEvent
                                                             source:source];
    session.animatesToStartingPositionsOnCancelOrFail = YES;
}

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

    // ── Build composite icon from RGBA buffer ────────────────────────────
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

    // ── Cursor position in view coordinates ──────────────────────────────
    NSPoint windowPos = [view.window mouseLocationOutsideOfEventStream];
    NSPoint viewPos = [view convertPoint:windowPos fromView:nil];

    // ── Icon frame centered on cursor ────────────────────────────────────
    NSSize imgSize = compositeIcon
        ? compositeIcon.size
        : NSMakeSize(64, 64);
    NSRect frame = NSMakeRect(
        viewPos.x - imgSize.width / 2,
        viewPos.y - imgSize.height / 2,
        imgSize.width,
        imgSize.height);

    // ── Collect file paths ───────────────────────────────────────────────
    NSMutableArray<NSString *> *filePathStrings = [NSMutableArray arrayWithCapacity:path_count];
    for (int i = 0; i < path_count; i++) {
        [filePathStrings addObject:[NSString stringWithUTF8String:paths[i]]];
    }

    // ── Build a SINGLE dragging item ─────────────────────────────────────
    // Using one NSDraggingItem prevents destination apps (Finder) from
    // replacing each item's image with individual file icons.
    NSDraggingItem *item;

    if (path_count == 1) {
        // Single file: use NSURL directly (modern, fully compatible)
        NSURL *fileURL = [NSURL fileURLWithPath:filePathStrings[0]];
        item = [[NSDraggingItem alloc] initWithPasteboardWriter:fileURL];
    } else {
        // Multiple files: write all paths to ONE pasteboard item via
        // NSFilenamesPboardType (plist array of path strings).
        // This gives us ONE drag item with ONE icon — no stacking.
        NSPasteboardItem *pbItem = [[NSPasteboardItem alloc] init];

        // NSFilenamesPboardType: Finder and most apps read this for multi-file drops
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        [pbItem setPropertyList:filePathStrings forType:NSFilenamesPboardType];
#pragma clang diagnostic pop

        // Also set the first file's URL for apps that only read public.file-url
        NSURL *firstURL = [NSURL fileURLWithPath:filePathStrings[0]];
        [pbItem setString:[firstURL absoluteString] forType:@"public.file-url"];

        item = [[NSDraggingItem alloc] initWithPasteboardWriter:pbItem];
    }

    [item setDraggingFrame:frame contents:(compositeIcon ?: [[NSImage alloc] initWithSize:imgSize])];

    // ── Synthetic mouse event for the drag session ───────────────────────
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

    // ── Start the drag ───────────────────────────────────────────────────
    PictoDragSource *source = [[PictoDragSource alloc] init];
    [view beginDraggingSessionWithItems:@[item]
                                  event:dragEvent
                                 source:source];
}

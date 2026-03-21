#import <Cocoa/Cocoa.h>

// ── Drag source ──────────────────────────────────────────────────────────

@interface PictoDragSource : NSObject <NSDraggingSource>
@end

@implementation PictoDragSource

- (NSDragOperation)draggingSession:(NSDraggingSession *)session
    sourceOperationMaskForDraggingContext:(NSDraggingContext)context {
    return NSDragOperationCopy;
}

// Required for the legacy dragImage:at:offset:event:pasteboard:source:slideBack: API
- (NSDragOperation)draggingSourceOperationMaskForLocal:(BOOL)flag {
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

    // Fallback icon if RGBA decode failed
    if (!compositeIcon) {
        compositeIcon = [[NSImage alloc] initWithSize:NSMakeSize(64, 64)];
    }

    // ── Collect file paths ───────────────────────────────────────────────
    NSMutableArray<NSString *> *filePathStrings = [NSMutableArray arrayWithCapacity:path_count];
    for (int i = 0; i < path_count; i++) {
        [filePathStrings addObject:[NSString stringWithUTF8String:paths[i]]];
    }

    // ── Cursor position ──────────────────────────────────────────────────
    NSPoint windowPos = [view.window mouseLocationOutsideOfEventStream];
    NSPoint viewPos = [view convertPoint:windowPos fromView:nil];

    // Position the drag image centered on cursor
    NSSize imgSize = compositeIcon.size;
    NSPoint dragPos = NSMakePoint(
        viewPos.x - imgSize.width / 2,
        viewPos.y - imgSize.height / 2);

    // ── Synthetic mouse event ────────────────────────────────────────────
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

    // ── Write ALL file paths to the drag pasteboard ──────────────────────
    // Using the legacy pasteboard-level API: one pasteboard with an array
    // of paths, one drag image. Finder reads NSFilenamesPboardType and
    // receives all files. No per-item NSDraggingItem stacking.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"

    NSPasteboard *pboard = [NSPasteboard pasteboardWithName:NSDragPboard];
    [pboard declareTypes:@[NSFilenamesPboardType] owner:nil];
    [pboard setPropertyList:filePathStrings forType:NSFilenamesPboardType];

    PictoDragSource *source = [[PictoDragSource alloc] init];

    [view dragImage:compositeIcon
                 at:dragPos
             offset:NSZeroSize
              event:dragEvent
         pasteboard:pboard
             source:source
          slideBack:YES];

#pragma clang diagnostic pop
}

#import <AppKit/AppKit.h>

static NSString *picto_png_data_url(NSImage *image) {
    if (image == nil) return nil;
    NSImage *copy = [image copy];
    copy.size = NSMakeSize(32.0, 32.0);
    NSData *tiff = copy.TIFFRepresentation;
    if (tiff == nil) return nil;
    NSBitmapImageRep *bitmap = [NSBitmapImageRep imageRepWithData:tiff];
    NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    if (png == nil) return nil;
    return [@"data:image/png;base64," stringByAppendingString:[png base64EncodedStringWithOptions:0]];
}

const char *picto_get_associated_applications(const char *file_path) {
    @autoreleasepool {
        if (file_path == NULL) return strdup("[]");
        NSString *path = [NSString stringWithUTF8String:file_path];
        NSURL *fileURL = [NSURL fileURLWithPath:path];
        NSWorkspace *workspace = NSWorkspace.sharedWorkspace;
        NSURL *defaultURL = [workspace URLForApplicationToOpenURL:fileURL];
        NSArray<NSURL *> *applicationURLs = [workspace URLsForApplicationsToOpenURL:fileURL];
        NSMutableArray *result = [NSMutableArray arrayWithCapacity:applicationURLs.count];

        for (NSURL *applicationURL in applicationURLs) {
            NSBundle *bundle = [NSBundle bundleWithURL:applicationURL];
            NSString *name = [bundle objectForInfoDictionaryKey:@"CFBundleDisplayName"]
                ?: [bundle objectForInfoDictionaryKey:@"CFBundleName"]
                ?: applicationURL.URLByDeletingPathExtension.lastPathComponent;
            NSString *bundleIdentifier = bundle.bundleIdentifier ?: @"";
            if ([bundleIdentifier isEqualToString:@"com.picto.desktop"]) continue;
            NSString *icon = picto_png_data_url([workspace iconForFile:applicationURL.path]);
            [result addObject:@{
                @"name": name,
                @"path": applicationURL.path,
                @"bundleIdentifier": bundleIdentifier,
                @"iconDataUrl": icon ?: [NSNull null],
                @"isDefault": @([applicationURL.path isEqualToString:defaultURL.path]),
            }];
        }

        [result sortUsingComparator:^NSComparisonResult(NSDictionary *left, NSDictionary *right) {
            BOOL leftDefault = [left[@"isDefault"] boolValue];
            BOOL rightDefault = [right[@"isDefault"] boolValue];
            if (leftDefault != rightDefault) return leftDefault ? NSOrderedAscending : NSOrderedDescending;
            return [left[@"name"] localizedCaseInsensitiveCompare:right[@"name"]];
        }];

        NSData *json = [NSJSONSerialization dataWithJSONObject:result options:0 error:nil];
        NSString *value = [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding] ?: @"[]";
        return strdup(value.UTF8String);
    }
}

void picto_free_string(const char *value) {
    free((void *)value);
}

bool picto_open_with_application(const char *application_path, const char *file_path) {
    @autoreleasepool {
        if (application_path == NULL || file_path == NULL) return false;
        NSURL *applicationURL = [NSURL fileURLWithPath:[NSString stringWithUTF8String:application_path]];
        NSURL *fileURL = [NSURL fileURLWithPath:[NSString stringWithUTF8String:file_path]];
        if (![applicationURL checkResourceIsReachableAndReturnError:nil] ||
            ![fileURL checkResourceIsReachableAndReturnError:nil]) return false;

        NSWorkspaceOpenConfiguration *configuration = [NSWorkspaceOpenConfiguration configuration];
        configuration.activates = YES;
        [NSWorkspace.sharedWorkspace openURLs:@[fileURL]
                         withApplicationAtURL:applicationURL
                                configuration:configuration
                            completionHandler:nil];
        return true;
    }
}

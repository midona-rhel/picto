# PBI-516: Electron security hardening — CSP and window.open

## AI-Generated Caveat
This PBI was produced by automated codebase analysis (2026-03-19). The security findings are based on static inspection of Electron configuration. The actual exploitability depends on what content is rendered and whether untrusted input reaches the renderer. Human review is recommended before acting on it.

## Priority
P1

## Problem
Two Electron security best practices are not currently implemented:

### 1. Missing Content-Security-Policy headers
None of the five HTML entry points (`index.html`, `detail.html`, `settings.html`, `subscriptions.html`, `library-manager.html`) include CSP meta tags. This means there is no browser-level restriction on script sources, style sources, or connection targets. While context isolation is enabled (good), CSP provides an important defense-in-depth layer against script injection.

### 2. Missing window.open handlers
`setWindowOpenHandler(() => ({ action: 'deny' }))` is only set on one window — the reverse image search popup in `electron/ipc/registerHandlers.mjs:89`. The main window, detail windows, settings window, subscriptions window, and library manager window do not have this handler. If any rendered content triggers `window.open()`, an uncontrolled Electron window could be created.

## Scope
- Add CSP meta tags to all HTML entry points
- Add `setWindowOpenHandler` to all window creation paths in `windowManager.mjs`

## Implementation

### CSP Headers
Add a `<meta>` tag to the `<head>` of each HTML file:
```html
<meta http-equiv="Content-Security-Policy"
      content="default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src media: data: blob: 'self'; media-src media:; connect-src 'self' ws://localhost:*; font-src 'self'">
```

Files to modify:
- `index.html`
- `detail.html`
- `settings.html`
- `subscriptions.html`
- `library-manager.html`

Note: `'unsafe-inline'` for styles is needed because Mantine injects inline styles. `ws://localhost:*` is needed for Vite HMR in dev mode — consider making this dev-only if possible.

### window.open Handlers
In `electron/windows/windowManager.mjs`, add to the `createWindow` function (or wherever `BrowserWindow` instances are created):
```javascript
win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
```

This should apply to all windows: main, detail, settings, subscriptions, library manager.

## Acceptance Criteria
1. Every HTML entry point has a CSP meta tag.
2. Opening DevTools in any window and checking the Console shows no CSP violations during normal use.
3. Running `window.open('about:blank')` in any window's DevTools Console is blocked.
4. The app functions normally — media protocol URLs load, thumbnails display, styles apply.

## Test Cases
1. Launch app → open DevTools → Console shows no CSP errors during normal browsing.
2. Open a detail window → media:// thumbnails and files load correctly.
3. In any window DevTools: `window.open('https://example.com')` → blocked (no new window).
4. Vite dev server HMR still works in development mode.

## Risk
Medium. CSP can be tricky — overly restrictive policies break functionality. The `'unsafe-inline'` for styles and `media:` scheme for images should cover Picto's usage, but edge cases (e.g., Mantine tooltip positioning, canvas operations) may need additional CSP directives. Testing should cover all major UI paths.

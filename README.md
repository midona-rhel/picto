# Picto

Desktop media library manager built with Electron + React + Rust core.

## Current Documentation Scope

Project docs are intentionally minimized for alpha prep.
Active documentation lives in:

1. [`docs/pbis/`](./docs/pbis/)
2. [`docs/pbis/active-alpha/`](./docs/pbis/active-alpha/)
3. [`docs/pbis/archive/`](./docs/pbis/archive/)

## Development

1. Install dependencies:
```bash
npm install
cd native/picto-node && npm install && cd ../..
```
2. Run app in dev:
```bash
npm run dev:electron
```

## Quality Gates

1. Alpha blocking gate:
```bash
npm run gate:alpha
```
2. Legacy non-blocking diagnostics:
```bash
npm run gate:legacy
```

## Packaging / Smoke

1. Local package:
```bash
npm run alpha:package
```
2. Local smoke:
```bash
npm run alpha:smoke -- --platform local --report artifacts/alpha-smoke/local.json
```

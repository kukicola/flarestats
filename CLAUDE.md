# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

FlareStats — a macOS menu bar app for Cloudflare Web Analytics. Built with Tauri 2 (Rust backend) + vanilla TypeScript frontend (no framework) + Vite bundler.

## Commands

```bash
npm run tauri dev          # Run app in development mode
npm run tauri build        # Build .app bundle (output: src-tauri/target/release/bundle/)
npm test                   # TypeScript tests (Vitest)
cargo test --manifest-path src-tauri/Cargo.toml   # Rust tests
npm run build              # Frontend-only build (tsc + vite build)
```

## Architecture

**Frontend (`src/`):** Vanilla TypeScript with direct DOM manipulation — no framework. All UI logic lives in `main.ts`, which renders two views (`dashboard` and `settings`) by replacing `app.innerHTML`. State is module-level variables. Chart.js 4 for stacked bar charts. Single CSS file with custom properties; dark mode via `dark` class on `<html>`.

**Backend (`src-tauri/src/`):**
- `main.rs` → delegates to `lib.rs` `run()`
- `lib.rs` — Tauri builder, tray icon/menu setup, NSPanel init/positioning via `tauri-nspanel` plugin
- `commands.rs` — all business logic: settings persistence (JSON in app data dir), Cloudflare REST API (site list), Cloudflare GraphQL API (analytics via `rumPageloadEventsAdaptiveGroups`), time series gap-filling. Sites are fetched concurrently with `futures::join_all`.

**IPC:** Frontend calls Rust via `invoke()` from `@tauri-apps/api/core` (`get_settings`, `save_settings`, `fetch_analytics`). Tray menu emits `open-settings` event listened to by the frontend.

## Key Patterns

- Settings auto-save on `change` events (no save button)
- Window is an NSPanel (via `tauri-nspanel`) so it appears over fullscreen apps; positioned manually below tray icon using rect from `TrayIconEvent`
- Panel toggles visibility on tray icon click; auto-refreshes data on DOM `focus` event (Tauri's `onFocusChanged` doesn't fire for NSPanel)
- Tray icon uses `icon_as_template(true)` — icon must be pure black with alpha=255 for correct macOS rendering
- Rust tests are inline `#[cfg(test)]` in `commands.rs`; TS tests in `utils.test.ts`
- No ESLint/Prettier configured
- CI runs on GitHub Actions (ubuntu-latest): `npm ci`, `npm test`, `npm run build`, `cargo test`
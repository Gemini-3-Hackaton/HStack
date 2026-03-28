# Tauri + React + Typescript

This template should help get you started developing with Tauri, React and Typescript in Vite.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## Development Helpers

To reset the welcome screen during development, run this from the repository root:

```bash
npm run reset:welcome
```

The script resolves the settings path from the Tauri app identifier, so it does not depend on a user-specific home directory layout.

## Official Cloud URL

The Tauri app now defaults its managed cloud connection to `https://hstack-private-api.onrender.com`.

That means the normal app workflows work without extra setup:

```bash
npm run dev
```

and Tauri builds keep the same default unless you explicitly override `VITE_OFFICIAL_CLOUD_URL`.

import { existsSync, readFileSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";

function getTauriIdentifier() {
  const configPath = path.resolve("crates/hstack-app/tauri.conf.json");
  const config = JSON.parse(readFileSync(configPath, "utf8"));

  if (!config.identifier || typeof config.identifier !== "string") {
    throw new Error(`Missing app identifier in ${configPath}`);
  }

  return config.identifier;
}

function getSettingsPath(identifier) {
  switch (process.platform) {
    case "darwin":
      return path.join(os.homedir(), "Library", "Application Support", identifier, "settings.json");
    case "linux": {
      const configHome = process.env.XDG_CONFIG_HOME || path.join(os.homedir(), ".config");
      return path.join(configHome, identifier, "settings.json");
    }
    default:
      throw new Error(`Unsupported platform: ${process.platform}. This helper currently supports macOS and Linux only.`);
  }
}

function main() {
  const identifier = getTauriIdentifier();
  const settingsPath = getSettingsPath(identifier);

  if (!existsSync(settingsPath)) {
    console.log(`No settings file found at ${settingsPath}`);
    return;
  }

  rmSync(settingsPath, { force: true });
  console.log(`Removed ${settingsPath}`);
}

main();
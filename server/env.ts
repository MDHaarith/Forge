import { existsSync } from "node:fs";
import os from "node:os";
import path from "node:path";

/**
 * Loads ~/.config/forge/env (or $FORGE_ENV_FILE) when the server is started, so the API key and
 * FORGE_* settings live in one private file instead of shell profiles or the desktop launcher.
 * Variables already set in the environment win. Imported first by index.ts because other modules
 * read their settings when they load.
 */
export const ENV_FILE = process.env.FORGE_ENV_FILE ?? path.join(os.homedir(), ".config", "forge", "env");

const startingServer = /server[/\\]index\.ts$/.test(process.argv[1] ?? "");
if (startingServer && existsSync(ENV_FILE)) process.loadEnvFile(ENV_FILE);

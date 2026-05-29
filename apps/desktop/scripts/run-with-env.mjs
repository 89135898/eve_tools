#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { spawn } from "node:child_process";

export async function loadEnvFiles(files, env, originalKeys, cwd = process.cwd()) {
  for (const file of files) {
    const path = resolve(cwd, file);
    let contents;
    try {
      contents = await readFile(path, "utf8");
    } catch (error) {
      if (error?.code === "ENOENT") {
        continue;
      }
      throw error;
    }

    for (const [key, value] of parseEnv(contents)) {
      if (!originalKeys.has(key)) {
        env[key] = value;
      }
    }
  }
}

export function parseEnv(contents) {
  const entries = [];
  for (const rawLine of contents.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) {
      continue;
    }

    const normalized = line.startsWith("export ") ? line.slice("export ".length).trim() : line;
    const separatorIndex = normalized.indexOf("=");
    if (separatorIndex <= 0) {
      continue;
    }

    const key = normalized.slice(0, separatorIndex).trim();
    const rawValue = normalized.slice(separatorIndex + 1).trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) {
      continue;
    }

    entries.push([key, unquote(rawValue)]);
  }
  return entries;
}

function unquote(value) {
  if (
    (value.startsWith("\"") && value.endsWith("\"")) ||
    (value.startsWith("'") && value.endsWith("'"))
  ) {
    return value.slice(1, -1);
  }
  return value;
}

function parseArgs(argv) {
  const envFiles = [];
  let commandIndex = argv.indexOf("--");
  if (commandIndex === -1) {
    throw new Error("Usage: run-with-env --env <file> [--env <file>] -- <command> [args...]");
  }

  for (let index = 0; index < commandIndex; index += 1) {
    if (argv[index] !== "--env" || !argv[index + 1]) {
      throw new Error("Usage: run-with-env --env <file> [--env <file>] -- <command> [args...]");
    }
    envFiles.push(argv[index + 1]);
    index += 1;
  }

  const command = argv[commandIndex + 1];
  const commandArgs = argv.slice(commandIndex + 2);
  if (!command) {
    throw new Error("Missing command after --");
  }

  return { envFiles, command, commandArgs };
}

async function main() {
  const { envFiles, command, commandArgs } = parseArgs(process.argv.slice(2));
  const env = { ...process.env };
  await loadEnvFiles(envFiles, env, new Set(Object.keys(process.env)));

  const child = spawn(command, commandArgs, {
    env,
    stdio: "inherit",
    shell: process.platform === "win32"
  });

  child.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 1);
  });
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}

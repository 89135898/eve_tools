import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { loadEnvFiles } from "./run-with-env.mjs";

test("loads API base URL from env file", async () => {
  const dir = await mkdtemp(join(tmpdir(), "evetools-env-"));
  try {
    await writeFile(
      join(dir, ".env.production"),
      "EVETOOLS_API_BASE_URL=http://13.158.109.65:8080\n",
      "utf8"
    );

    const env = {};
    await loadEnvFiles([".env.production"], env, new Set(), dir);

    assert.equal(env.EVETOOLS_API_BASE_URL, "http://13.158.109.65:8080");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("keeps shell environment over env file values", async () => {
  const dir = await mkdtemp(join(tmpdir(), "evetools-env-"));
  try {
    await writeFile(
      join(dir, ".env.production"),
      "EVETOOLS_API_BASE_URL=http://13.158.109.65:8080\n",
      "utf8"
    );

    const env = { EVETOOLS_API_BASE_URL: "http://127.0.0.1:8080" };
    await loadEnvFiles([".env.production"], env, new Set(["EVETOOLS_API_BASE_URL"]), dir);

    assert.equal(env.EVETOOLS_API_BASE_URL, "http://127.0.0.1:8080");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("allows local env file to override production defaults", async () => {
  const dir = await mkdtemp(join(tmpdir(), "evetools-env-"));
  try {
    await writeFile(
      join(dir, ".env.production"),
      "EVETOOLS_API_BASE_URL=http://13.158.109.65:8080\n",
      "utf8"
    );
    await writeFile(
      join(dir, ".env.local"),
      "EVETOOLS_API_BASE_URL=http://127.0.0.1:8080\n",
      "utf8"
    );

    const env = {};
    await loadEnvFiles([".env.production", ".env.local"], env, new Set(), dir);

    assert.equal(env.EVETOOLS_API_BASE_URL, "http://127.0.0.1:8080");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

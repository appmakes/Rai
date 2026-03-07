#!/usr/bin/env node
"use strict";

const os = require("os");
const fs = require("fs");
const path = require("path");
const https = require("https");
const { execSync } = require("child_process");

const VERSION = require("./package.json").version;
const REPO = "appmakes/Rai";
const BIN_DIR = path.join(__dirname, "bin");

const PLATFORM_MAP = {
  "darwin-x64": { artifact: "rai-x86_64-apple-darwin.tar.gz", binary: "rai" },
  "darwin-arm64": { artifact: "rai-aarch64-apple-darwin.tar.gz", binary: "rai" },
  "linux-x64": { artifact: "rai-x86_64-linux-gnu.tar.gz", binary: "rai" },
  "linux-arm64": { artifact: "rai-aarch64-linux-gnu.tar.gz", binary: "rai" },
  "win32-x64": { artifact: "rai-x86_64-pc-windows-msvc.zip", binary: "rai.exe" },
};

function getPlatformKey() {
  const platform = os.platform();
  const arch = os.arch();
  return `${platform}-${arch}`;
}

function fetch(url) {
  return new Promise((resolve, reject) => {
    https.get(url, { headers: { "User-Agent": "rai-cli-npm" } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return fetch(res.headers.location).then(resolve, reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
      }
      const chunks = [];
      res.on("data", (chunk) => chunks.push(chunk));
      res.on("end", () => resolve(Buffer.concat(chunks)));
      res.on("error", reject);
    }).on("error", reject);
  });
}

async function extract(buffer, artifact, binaryName) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "rai-"));
  const archivePath = path.join(tmpDir, artifact);
  fs.writeFileSync(archivePath, buffer);

  if (artifact.endsWith(".tar.gz")) {
    execSync(`tar xzf "${archivePath}" -C "${tmpDir}"`);
  } else if (artifact.endsWith(".zip")) {
    if (os.platform() === "win32") {
      execSync(`powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${tmpDir}'"`, { stdio: "ignore" });
    } else {
      execSync(`unzip -o "${archivePath}" -d "${tmpDir}"`, { stdio: "ignore" });
    }
  }

  const src = path.join(tmpDir, binaryName);
  const dest = path.join(BIN_DIR, binaryName);

  if (!fs.existsSync(src)) {
    throw new Error(`Expected binary not found: ${src}`);
  }

  fs.mkdirSync(BIN_DIR, { recursive: true });
  fs.copyFileSync(src, dest);
  fs.chmodSync(dest, 0o755);

  fs.rmSync(tmpDir, { recursive: true, force: true });
}

async function main() {
  const key = getPlatformKey();
  const target = PLATFORM_MAP[key];

  if (!target) {
    console.error(`Unsupported platform: ${key}`);
    console.error(`Supported: ${Object.keys(PLATFORM_MAP).join(", ")}`);
    process.exit(1);
  }

  const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${target.artifact}`;
  console.log(`Downloading rai v${VERSION} for ${key}...`);

  try {
    const buffer = await fetch(url);
    await extract(buffer, target.artifact, target.binary);
    console.log(`rai v${VERSION} installed successfully.`);
  } catch (err) {
    console.error(`Failed to install rai: ${err.message}`);
    process.exit(1);
  }
}

main();

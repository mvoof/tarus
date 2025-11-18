const fs = require('fs');
const path = require('path');

const serverDir = path.join(
  __dirname,
  '..',
  '..',
  'lsp-server',
  'target',
  'release'
);

const clientBinDir = path.join(__dirname, '..', 'bin');

let binaryName;
let targetName;

const platform = process.platform;
const arch = process.arch;

if (platform === 'win32') {
  binaryName = 'lsp-server.exe';
  targetName = 'lsp-server-win-x64.exe';
} else if (platform === 'darwin') {
  binaryName = 'lsp-server';
  targetName =
    arch === 'arm64' ? 'lsp-server-macos-arm64' : 'lsp-server-macos-x64';
} else {
  // Linux
  binaryName = 'lsp-server';
  targetName = 'lsp-server-linux-x64';
}

const src = path.join(serverDir, binaryName);
const dest = path.join(clientBinDir, targetName);

console.log(`[Copy Script] Source: ${src}`);
console.log(`[Copy Script] Dest:   ${dest}`);

if (!fs.existsSync(src)) {
  console.error(`\n❌ ERROR: Source binary not found at ${src}`);
  console.error('   Run "cargo build --release" in lsp-server folder first.\n');
  process.exit(1);
}

if (!fs.existsSync(clientBinDir)) {
  console.log(`[Copy Script] Creating directory: ${clientBinDir}`);
  fs.mkdirSync(clientBinDir, { recursive: true });
}

if (fs.existsSync(dest)) {
  try {
    fs.unlinkSync(dest);
    console.log('[Copy Script] Old binary removed.');
  } catch (err) {
    console.warn(
      `[Copy Script] ⚠️ Warning: Could not unlink old binary: ${err.message}`
    );
  }
}

try {
  fs.copyFileSync(src, dest);
  console.log('✅ LSP binary copied successfully.');

  if (platform !== 'win32') {
    fs.chmodSync(dest, '755');
  }
} catch (err) {
  console.error(`❌ Error copying file: ${err.message}`);
  process.exit(1);
}

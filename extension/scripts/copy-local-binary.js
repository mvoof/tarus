const fs = require('fs');
const path = require('path');

const serverTargetDir = path.join(
  __dirname,
  '..',
  '..',
  'lsp-server',
  'target',
  'release'
);

const configSrc = path.join(
  __dirname,
  '..',
  '..',
  'lsp-server',
  'src',
  'command_syntax.json'
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

const binSrc = path.join(serverTargetDir, binaryName);
const binDest = path.join(clientBinDir, targetName);
const configDest = path.join(clientBinDir, 'command_syntax.json');

console.log(`[Copy Script] Binary Source: ${binSrc}`);
console.log(`[Copy Script] Config Source: ${configSrc}`);
console.log(`[Copy Script] Destination Dir: ${clientBinDir}`);

if (!fs.existsSync(binSrc)) {
  console.error(`\n❌ ERROR: Source binary not found at ${binSrc}`);
  console.error('   Run "cargo build --release" in lsp-server folder first.\n');
  process.exit(1);
}

if (!fs.existsSync(configSrc)) {
  console.error(`\n❌ ERROR: Config file not found at ${configSrc}`);
  console.error(
    '   Make sure command_syntax.json is in the lsp-server root folder.\n'
  );
  process.exit(1);
}

if (!fs.existsSync(clientBinDir)) {
  console.log(`[Copy Script] Creating directory: ${clientBinDir}`);
  fs.mkdirSync(clientBinDir, { recursive: true });
}

if (fs.existsSync(binDest)) {
  try {
    fs.unlinkSync(binDest);
    console.log('[Copy Script] Old binary removed.');
  } catch (err) {
    console.warn(
      `[Copy Script] ⚠️ Warning: Could not unlink old binary: ${err.message}`
    );
  }
}

try {
  fs.copyFileSync(binSrc, binDest);
  console.log('✅ LSP binary copied successfully.');

  if (platform !== 'win32') {
    fs.chmodSync(binDest, '755');
  }
} catch (err) {
  console.error(`❌ Error copying binary: ${err.message}`);
  process.exit(1);
}

try {
  if (fs.existsSync(configDest)) {
    fs.unlinkSync(configDest);
  }
  fs.copyFileSync(configSrc, configDest);
  console.log('✅ Syntax config copied successfully.');
} catch (err) {
  console.error(`❌ Error copying config: ${err.message}`);
  process.exit(1);
}

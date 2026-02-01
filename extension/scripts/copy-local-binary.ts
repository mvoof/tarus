import * as fs from 'fs';
import * as path from 'path';
import { getBinaryNames } from '../src/utils/platform-utils';

const serverTargetDir = path.join(
  __dirname,
  '..',
  '..',
  '..',
  'lsp-server',
  'target',
  'release'
);

const clientBinDir = path.join(__dirname, '..', '..', 'bin');

// Get platform-specific binary names using shared utility
const { source: binaryName, target: targetName } = getBinaryNames();

const binSrc = path.join(serverTargetDir, binaryName);
const binDest = path.join(clientBinDir, targetName);

console.log(`[Copy Script] Binary Source: ${binSrc}`);
console.log(`[Copy Script] Destination Dir: ${clientBinDir}`);

if (!fs.existsSync(binSrc)) {
  console.error(`\n❌ ERROR: Source binary not found at ${binSrc}`);
  console.error('   Run "cargo build --release" in lsp-server folder first.\n');

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
    const error = err as NodeJS.ErrnoException;

    console.warn(
      `[Copy Script] ⚠️ Warning: Could not unlink old binary: ${error.message}`
    );
  }
}

try {
  fs.copyFileSync(binSrc, binDest);

  console.log('✅ LSP binary copied successfully.');

  // Set executable permissions on Unix-like systems
  if (process.platform !== 'win32') {
    fs.chmodSync(binDest, '755');
  }
} catch (err) {
  const error = err as NodeJS.ErrnoException;

  console.error(`❌ Error copying binary: ${error.message}`);

  process.exit(1);
}

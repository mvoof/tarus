/**
 * Platform-specific binary naming utilities for TARUS LSP Server
 * Centralizes binary name resolution to avoid code duplication
 */

const SERVER_NAME = 'lsp-server';

/**
 * Binary names for source (Cargo output) and target (extension bin folder)
 */
export interface BinaryNames {
  source: string;
  target: string;
}

/**
 * Get platform-specific binary names
 * @param platform - Platform identifier (win32, darwin, linux)
 * @param arch - Architecture identifier (x64, arm64)
 * @returns Binary names for source (Cargo output) and target (extension bin folder)
 */
export function getBinaryNames(
  platform: string = process.platform,
  arch: string = process.arch
): BinaryNames {
  let sourceName: string;
  let targetName: string;

  if (platform === 'win32') {
    sourceName = `${SERVER_NAME}.exe`;
    targetName = `${SERVER_NAME}-win-x64.exe`;
  } else if (platform === 'darwin') {
    sourceName = SERVER_NAME;
    targetName =
      arch === 'arm64'
        ? `${SERVER_NAME}-macos-arm64`
        : `${SERVER_NAME}-macos-x64`;
  } else {
    // Linux and other Unix-like systems
    sourceName = SERVER_NAME;
    targetName = `${SERVER_NAME}-linux-x64`;
  }

  return {
    source: sourceName,
    target: targetName,
  };
}

/**
 * Get the target binary name for the extension (used in bin/ folder)
 * @param platform - Platform identifier
 * @param arch - Architecture identifier
 * @returns Target binary name
 */
export function getTargetBinaryName(
  platform: string = process.platform,
  arch: string = process.arch
): string {
  return getBinaryNames(platform, arch).target;
}

/**
 * Get the source binary name (Cargo build output)
 * @param platform - Platform identifier
 * @returns Source binary name
 */
export function getSourceBinaryName(
  platform: string = process.platform
): string {
  return getBinaryNames(platform).source;
}

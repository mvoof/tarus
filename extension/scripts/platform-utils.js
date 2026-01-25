/**
 * Platform-specific binary naming utilities for TARUS LSP Server
 * Centralizes binary name resolution to avoid code duplication
 */

const SERVER_NAME = 'lsp-server';

/**
 * Get platform-specific binary names
 * @param {string} [platform=process.platform] - Platform identifier (win32, darwin, linux)
 * @param {string} [arch=process.arch] - Architecture identifier (x64, arm64)
 * @returns {{source: string, target: string}} Binary names for source (Cargo output) and target (extension bin folder)
 */
function getBinaryNames(platform = process.platform, arch = process.arch) {
  let sourceName;
  let targetName;

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
 * @param {string} [platform=process.platform] - Platform identifier
 * @param {string} [arch=process.arch] - Architecture identifier
 * @returns {string} Target binary name
 */
function getTargetBinaryName(platform = process.platform, arch = process.arch) {
  return getBinaryNames(platform, arch).target;
}

/**
 * Get the source binary name (Cargo build output)
 * @param {string} [platform=process.platform] - Platform identifier
 * @returns {string} Source binary name
 */
function getSourceBinaryName(platform = process.platform) {
  return getBinaryNames(platform).source;
}

module.exports = {
  getBinaryNames,
  getTargetBinaryName,
  getSourceBinaryName,
};

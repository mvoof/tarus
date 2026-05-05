import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';
import process from 'node:process';

const newVersion = process.argv[2];

if (!newVersion) {
  console.error(
    'Please provide a version. Example: npm run version-bump 1.2.3'
  );
  process.exit(1);
}

if (!/^\d+\.\d+\.\d+(-[a-zA-Z0-9.]+)?$/.test(newVersion)) {
  console.error(
    `Invalid version format: ${newVersion}. Use semver (e.g., 1.2.3 or 1.2.3-beta.1)`
  );
  process.exit(1);
}

const extensionDir = path.resolve(process.cwd());
const rootDir = path.resolve(extensionDir, '..');

const paths = {
  packageJson: path.join(extensionDir, 'package.json'),
  cargoToml: path.join(rootDir, 'lsp-server', 'Cargo.toml'),
};

// 1. Update package.json
console.log(`Updating package.json to ${newVersion}...`);
const pkg = JSON.parse(fs.readFileSync(paths.packageJson, 'utf-8'));
pkg.version = newVersion;
fs.writeFileSync(paths.packageJson, JSON.stringify(pkg, null, 2) + '\n');

// 2. Update package-lock.json
console.log('Updating package-lock.json...');
try {
  execSync('npm install --package-lock-only', {
    cwd: extensionDir,
    stdio: 'inherit',
  });
} catch {
  console.warn('Failed to update package-lock.json automatically.');
}

// 3. Update Cargo.toml
console.log(`Updating Cargo.toml to ${newVersion}...`);
let cargoToml = fs.readFileSync(paths.cargoToml, 'utf-8');
const updatedCargoToml = cargoToml.replace(
  /(\[package\](?:(?!^\[)[\s\S])*?^\s*version\s*=\s*")[^"]*(")/m,
  `$1${newVersion}$2`
);

if (updatedCargoToml === cargoToml) {
  console.error(
    'Failed to update version in Cargo.toml. Ensure [package] has a version field.'
  );
  process.exit(1);
}
fs.writeFileSync(paths.cargoToml, updatedCargoToml);

// 4. Update Cargo.lock
console.log('Updating Cargo.lock...');
try {
  execSync('cargo update -p lsp-server', {
    cwd: path.join(rootDir, 'lsp-server'),
    stdio: 'inherit',
  });
} catch {
  console.warn(
    'Failed to update Cargo.lock. Run "cargo check" in lsp-server/ manually.'
  );
}

console.log(`\nSuccessfully bumped version to ${newVersion}.`);

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const frontendDir = path.resolve(scriptDir, '..', 'frontend');
const publicDir = path.join(frontendDir, 'public');
const distDir = path.join(frontendDir, 'dist');

// Copy by reading and writing instead of fs.copyFileSync: 9p checkouts reject
// copyfile(2) with EPERM (see CONTRIBUTING.md), while plain reads and writes
// work everywhere.
function copyFile(sourcePath, targetPath) {
  fs.writeFileSync(targetPath, fs.readFileSync(sourcePath));
}

function copyDirectory(source, target) {
  fs.mkdirSync(target, { recursive: true });
  for (const entry of fs.readdirSync(source, { withFileTypes: true })) {
    const sourcePath = path.join(source, entry.name);
    const targetPath = path.join(target, entry.name);
    if (entry.isDirectory()) {
      copyDirectory(sourcePath, targetPath);
    } else {
      copyFile(sourcePath, targetPath);
    }
  }
}

if (!fs.existsSync(publicDir)) {
  console.error(`public directory not found at ${publicDir}`);
  process.exit(1);
}

copyDirectory(publicDir, distDir);
console.log(`Copied public assets to ${distDir}`);

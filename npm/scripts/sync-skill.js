const fs = require('node:fs');
const path = require('node:path');

const packageRoot = path.resolve(__dirname, '..');
const repositoryRoot = path.resolve(packageRoot, '..');
const sourcePath = path.join(repositoryRoot, 'SKILL.md');
const destinationPath = path.join(packageRoot, 'SKILL.md');

if (!fs.existsSync(sourcePath)) {
    throw new Error(`SKILL.md source is missing: ${sourcePath}`);
}

fs.copyFileSync(sourcePath, destinationPath);

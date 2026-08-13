#!/usr/bin/env node

const fs = require('node:fs');
const path = require('node:path');

const repositoryRoot = path.resolve(__dirname, '..');
const packageFiles = [
    'core/Cargo.toml',
    'cli/Cargo.toml',
    'mcp/Cargo.toml'
];

function cargoLockVersion(packageName) {
    const content = fs.readFileSync(path.join(repositoryRoot, 'Cargo.lock'), 'utf8');
    const packagePattern = new RegExp(
        `\\[\\[package\\]\\]\\nname = "${packageName}"\\nversion = "([^"]+)"`,
        'm'
    );
    const match = content.match(packagePattern);
    if (!match) {
        throw new Error(`Package ${packageName} is missing from Cargo.lock`);
    }
    return match[1];
}

function cargoVersion(relativePath) {
    const content = fs.readFileSync(path.join(repositoryRoot, relativePath), 'utf8');
    const match = content.match(/^version\s*=\s*"([^"]+)"/m);
    if (!match) {
        throw new Error(`Package version is missing from ${relativePath}`);
    }
    return match[1];
}

const npmPackage = JSON.parse(
    fs.readFileSync(path.join(repositoryRoot, 'npm/package.json'), 'utf8')
);
const npmLock = JSON.parse(
    fs.readFileSync(path.join(repositoryRoot, 'npm/package-lock.json'), 'utf8')
);
const versions = new Map(packageFiles.map((file) => [file, cargoVersion(file)]));
versions.set('npm/package.json', npmPackage.version);
versions.set('npm/package-lock.json', npmLock.version);
versions.set('Cargo.lock:ccm-core', cargoLockVersion('ccm-core'));
versions.set('Cargo.lock:ccm-cli', cargoLockVersion('ccm-cli'));
versions.set('Cargo.lock:ccm-mcp', cargoLockVersion('ccm-mcp'));

const uniqueVersions = new Set(versions.values());
if (uniqueVersions.size !== 1) {
    throw new Error(
        `Release versions differ: ${[...versions].map(([file, version]) => `${file}=${version}`).join(', ')}`
    );
}

const version = uniqueVersions.values().next().value;
const tag = process.argv[2];
if (tag && tag !== `v${version}`) {
    throw new Error(`Release tag ${tag} does not match package version v${version}`);
}

console.log(`Release versions are consistent: v${version}`);

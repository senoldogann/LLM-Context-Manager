#!/usr/bin/env node

const { spawn, spawnSync } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');
const https = require('https');
const crypto = require('crypto');

function resolvePackageVersion() {
    if (process.env.CCM_BINARY_VERSION && process.env.CCM_BINARY_VERSION.trim() !== '') {
        return process.env.CCM_BINARY_VERSION.trim();
    }

    if (process.env.npm_package_version && process.env.npm_package_version.trim() !== '') {
        return process.env.npm_package_version.trim();
    }

    try {
        const pkgPath = path.join(__dirname, '..', 'package.json');
        const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
        if (pkg.version && String(pkg.version).trim() !== '') {
            return String(pkg.version).trim();
        }
    } catch (error) {
        console.warn('[CCM] Failed to resolve package version from package.json.');
    }

    throw new Error('Unable to resolve package version for binary download.');
}

const VERSION = resolvePackageVersion();
const REPO = 'senoldogann/LLM-Context-Manager';
const BIN_DIR = path.join(os.homedir(), '.ccm', 'bin');
const CHECKSUMS_FILE = 'checksums.txt';
let checksumCache = null;

const MCP_SERVER_NAME = 'context-manager';
const MCP_COMMAND = 'npx';
const MCP_ARGS = ['-y', '@senoldogann/context-manager', 'mcp'];
const MCP_ENV = {
    RUST_LOG: 'info'
};
const ALLOWED_REDIRECT_HOSTS = new Set([
    'github.com',
    'objects.githubusercontent.com',
    'release-assets.githubusercontent.com'
]);

function allowUnverifiedBinaries() {
    const raw = process.env.CCM_ALLOW_UNVERIFIED_BINARIES || process.env.CCM_SKIP_CHECKSUM || '';
    return ['1', 'true', 'yes'].includes(raw.toLowerCase());
}

async function installMcp() {
    const home = os.homedir();
    const jsonTargets = [];

    if (os.platform() === 'darwin') {
        jsonTargets.push(path.join(home, 'Library', 'Application Support', 'Claude', 'claude_desktop_config.json'));
        jsonTargets.push(path.join(home, '.gemini', 'antigravity', 'mcp_config.json'));
        jsonTargets.push(path.join(home, 'Library', 'Application Support', 'Code', 'User', 'globalStorage', 'saoudrizwan.claude-dev', 'settings', 'cline_mcp_settings.json'));
        jsonTargets.push(path.join(home, 'Library', 'Application Support', 'Code', 'User', 'globalStorage', 'rooveterinaryinc.roo-cline', 'settings', 'cline_mcp_settings.json'));
    } else if (os.platform() === 'win32') {
        const appData = process.env.APPDATA || '';
        jsonTargets.push(path.join(appData, 'Claude', 'claude_desktop_config.json'));
        jsonTargets.push(path.join(process.env.USERPROFILE || '', 'AppData', 'Roaming', 'Code', 'User', 'globalStorage', 'saoudrizwan.claude-dev', 'settings', 'cline_mcp_settings.json'));
    } else if (os.platform() === 'linux') {
        jsonTargets.push(path.join(home, '.config', 'Claude', 'claude_desktop_config.json'));
        jsonTargets.push(path.join(home, '.config', 'Code', 'User', 'globalStorage', 'saoudrizwan.claude-dev', 'settings', 'cline_mcp_settings.json'));
    }

    jsonTargets.push(path.join(home, '.cursor', 'mcp.json'));

    const mcpConfig = {
        command: MCP_COMMAND,
        args: MCP_ARGS,
        env: MCP_ENV
    };

    console.log("[CCM] Pre-downloading binaries for all tools...");
    await getBinaryFor('ccm-cli');
    await getBinaryFor('ccm-mcp');

    let installedCount = 0;

    for (const configPath of jsonTargets) {
        if (installJsonConfig(configPath, mcpConfig)) {
            installedCount++;
        }
    }

    if (installCodexConfig()) {
        installedCount++;
    }

    if (installedCount === 0) {
        console.log("[CCM] No supported MCP config directories found.");
        console.log("[CCM] Add this server manually:");
        console.log(JSON.stringify({ [MCP_SERVER_NAME]: mcpConfig }, null, 2));
        console.log("[CCM] Codex example:");
        console.log(`codex mcp add ${MCP_SERVER_NAME} --env RUST_LOG=info -- ${MCP_COMMAND} ${MCP_ARGS.join(' ')}`);
    } else {
        console.log("[CCM] Installation complete! Restart your AI editor to see the changes.");
    }
}

function installJsonConfig(configPath, mcpConfig) {
    const dir = path.dirname(configPath);

    if (!fs.existsSync(dir)) {
        return false;
    }

    let config = { mcpServers: {} };
    if (fs.existsSync(configPath)) {
        try {
            const content = fs.readFileSync(configPath, 'utf8');
            config = JSON.parse(content);
            fs.copyFileSync(configPath, `${configPath}.bak`);
        } catch (e) {
            console.warn(`[CCM] Could not parse ${configPath}, creating backup and starting fresh section.`);
        }
    }

    if (!config.mcpServers || typeof config.mcpServers !== 'object') {
        config.mcpServers = {};
    }

    config.mcpServers[MCP_SERVER_NAME] = mcpConfig;
    fs.writeFileSync(configPath, JSON.stringify(config, null, 2));
    console.log(`[CCM] ✓ Successfully updated: ${configPath}`);
    return true;
}

function installCodexConfig() {
    const listResult = spawnSync('codex', ['mcp', 'list', '--json'], {
        encoding: 'utf8'
    });

    if (listResult.error) {
        return false;
    }

    if (listResult.status !== 0) {
        console.warn(`[CCM] Codex MCP inspection failed: ${listResult.stderr.trim()}`);
        return false;
    }

    let existingServers = [];
    try {
        existingServers = JSON.parse(listResult.stdout);
    } catch (error) {
        console.warn('[CCM] Could not parse Codex MCP list output.');
        return false;
    }

    const existing = existingServers.find((server) => server.name === MCP_SERVER_NAME);
    if (existing) {
        const removeResult = spawnSync('codex', ['mcp', 'remove', MCP_SERVER_NAME], {
            encoding: 'utf8'
        });

        if (removeResult.status !== 0) {
            console.warn(`[CCM] Codex MCP removal failed: ${removeResult.stderr.trim()}`);
            return false;
        }
    }

    const addArgs = ['mcp', 'add', MCP_SERVER_NAME, '--env', 'RUST_LOG=info', '--', MCP_COMMAND, ...MCP_ARGS];
    const addResult = spawnSync('codex', addArgs, {
        encoding: 'utf8'
    });

    if (addResult.status !== 0) {
        console.warn(`[CCM] Codex MCP install failed: ${addResult.stderr.trim()}`);
        return false;
    }

    console.log('[CCM] ✓ Successfully updated: ~/.codex/config.toml');
    return true;
}

function createUniqueTmpPath(binPath) {
    return `${binPath}.${process.pid}.${Date.now()}.tmp`;
}

async function getBinaryFor(commandName) {
    const platform = os.platform();
    const arch = os.arch();

    let target = '';
    if (platform === 'darwin') {
        target = arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
    } else if (platform === 'linux') {
        target = 'x86_64-unknown-linux-gnu';
    } else if (platform === 'win32') {
        target = 'x86_64-pc-windows-msvc.exe';
    }

    const binFilename = `${commandName}-v${VERSION}-${target}`;
    const binPath = path.join(BIN_DIR, binFilename);
    const remoteFilename = `${commandName}-${target}`;

    // If file exists, ensure it is executable
    if (fs.existsSync(binPath)) {
        try {
            fs.chmodSync(binPath, '755');
            return binPath;
        } catch (e) {
            // If chmod fails, maybe it's a broken file, try to redownload
        }
    }

    console.log(`[CCM] Downloading ${commandName} v${VERSION} for ${target}...`);

    if (!fs.existsSync(BIN_DIR)) {
        fs.mkdirSync(BIN_DIR, { recursive: true });
    }

    const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${commandName}-${target}`;
    const tmpPath = createUniqueTmpPath(binPath);

    try {
        await downloadFile(url, tmpPath);
        await verifyChecksum(tmpPath, [remoteFilename, binFilename]);
        fs.chmodSync(tmpPath, '755');
        if (fs.existsSync(binPath)) {
            fs.unlinkSync(tmpPath);
            return binPath;
        }
        fs.renameSync(tmpPath, binPath);
    } catch (err) {
        if (fs.existsSync(tmpPath)) fs.unlinkSync(tmpPath);
        throw err;
    }

    return binPath;
}

async function getBinary() {
    // Detect which tool to run
    let commandName = 'ccm-cli';
    const binName = path.basename(process.argv[1]);

    if (binName.includes('mcp')) {
        commandName = 'ccm-mcp';
    } else if (process.argv[2] === 'mcp') {
        commandName = 'ccm-mcp';
        process.argv.splice(2, 1);
    } else if (process.argv[2] === 'install') {
        await installMcp();
        process.exit(0);
    }

    return await getBinaryFor(commandName);
}

function downloadFile(url, dest) {
    return new Promise((resolve, reject) => {
        fs.mkdirSync(path.dirname(dest), { recursive: true });
        https.get(url, (response) => {
            if (response.statusCode === 302 || response.statusCode === 301) {
                try {
                    const redirectUrl = resolveRedirectUrl(url, response.headers.location);
                    return downloadFile(redirectUrl, dest).then(resolve).catch(reject);
                } catch (error) {
                    return reject(error);
                }
            }
            if (response.statusCode !== 200) {
                return reject(new Error(`Failed to download: ${response.statusCode}`));
            }
            const file = fs.createWriteStream(dest);
            response.pipe(file);
            file.on('finish', () => {
                file.close(resolve);
            });
            file.on('error', (err) => {
                fs.unlink(dest, () => { });
                reject(err);
            });
        }).on('error', (err) => {
            fs.unlink(dest, () => { });
            reject(err);
        });
    });
}

function downloadText(url) {
    return new Promise((resolve, reject) => {
        https.get(url, (response) => {
            if (response.statusCode === 302 || response.statusCode === 301) {
                try {
                    const redirectUrl = resolveRedirectUrl(url, response.headers.location);
                    return downloadText(redirectUrl).then(resolve).catch(reject);
                } catch (error) {
                    return reject(error);
                }
            }
            if (response.statusCode !== 200) {
                return reject(new Error(`Failed to download: ${response.statusCode}`));
            }
            let data = '';
            response.setEncoding('utf8');
            response.on('data', (chunk) => {
                data += chunk;
            });
            response.on('end', () => resolve(data));
        }).on('error', reject);
    });
}

async function getChecksums() {
    if (checksumCache) return checksumCache;

    const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${CHECKSUMS_FILE}`;
    try {
        const text = await downloadText(url);
        checksumCache = parseChecksums(text);
        return checksumCache;
    } catch (err) {
        return null;
    }
}

function resolveRedirectUrl(sourceUrl, location) {
    if (!location) {
        throw new Error('Redirect response did not include a Location header');
    }

    const resolved = new URL(location, sourceUrl);
    if (resolved.protocol !== 'https:') {
        throw new Error(`Blocked redirect to non-HTTPS URL: ${resolved.href}`);
    }
    if (!ALLOWED_REDIRECT_HOSTS.has(resolved.hostname)) {
        throw new Error(`Blocked redirect to unexpected host: ${resolved.hostname}`);
    }

    return resolved.toString();
}

function parseChecksums(text) {
    const map = new Map();
    for (const line of text.split('\n')) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        const parts = trimmed.split(/\s+/);
        if (parts.length < 2) continue;
        const hash = parts[0].toLowerCase();
        const filename = parts[parts.length - 1].replace(/^\*/, '');
        map.set(filename, hash);
    }
    return map;
}

function sha256File(filePath) {
    return new Promise((resolve, reject) => {
        const hash = crypto.createHash('sha256');
        const stream = fs.createReadStream(filePath);
        stream.on('error', reject);
        stream.on('data', (data) => hash.update(data));
        stream.on('end', () => resolve(hash.digest('hex')));
    });
}

async function verifyChecksum(filePath, candidates) {
    const checksums = await getChecksums();
    if (!checksums) {
        if (allowUnverifiedBinaries()) {
            console.warn('[CCM] Checksum manifest not found. Proceeding without verification.');
            return;
        }
        throw new Error('Checksum manifest not found. Set CCM_ALLOW_UNVERIFIED_BINARIES=1 to bypass.');
    }

    const expected = candidates
        .map((name) => [name, `${name}.exe`])
        .flat()
        .map((name) => checksums.get(name))
        .find(Boolean);
    if (!expected) {
        if (allowUnverifiedBinaries()) {
            console.warn('[CCM] Checksum not found for binary. Proceeding without verification.');
            return;
        }
        throw new Error('Checksum not found for binary. Set CCM_ALLOW_UNVERIFIED_BINARIES=1 to bypass.');
    }

    const actual = await sha256File(filePath);
    if (actual !== expected) {
        throw new Error(`Checksum mismatch. Expected ${expected}, got ${actual}`);
    }
}

async function main() {
    try {
        const binPath = await getBinary();
        const args = process.argv.slice(2);

        const child = spawn(binPath, args, {
            stdio: 'inherit',
            env: {
                ...process.env,
                CCM_PROJECT_ROOT: process.env.CCM_PROJECT_ROOT || process.cwd()
            }
        });

        child.on('exit', (code) => {
            process.exit(code);
        });
    } catch (err) {
        console.error(`[CCM Error] ${err.message}`);
        process.exit(1);
    }
}

main();

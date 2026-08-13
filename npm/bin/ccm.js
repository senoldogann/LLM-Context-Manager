#!/usr/bin/env node

const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');
const https = require('https');
const crypto = require('crypto');
const zlib = require('zlib');
const { pipeline } = require('stream/promises');

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
const DOWNLOAD_TIMEOUT_MS = positiveInteger(process.env.CCM_DOWNLOAD_TIMEOUT_MS, 120_000);
const DOWNLOAD_ATTEMPTS = positiveInteger(process.env.CCM_DOWNLOAD_ATTEMPTS, 3);
let checksumCache = null;

const MCP_SERVER_NAME = 'context-manager';
const MCP_COMMAND = 'npx';
const MCP_ARGS = ['-y', `@senoldogann/context-manager@${VERSION}`, 'mcp'];
// Allowlist kuruluma eklenir: MCP sunucusu yalnızca kurulum dizinindeki
// projeye erişebilir. Geniş erişim gerekiyorsa CCM_ALLOWED_ROOTS genişletilir.
const INSTALL_PROJECT_ROOT = process.cwd();
const MCP_ENV = {
    RUST_LOG: 'info',
    CCM_PROJECT_ROOT: INSTALL_PROJECT_ROOT,
    CCM_ALLOWED_ROOTS: INSTALL_PROJECT_ROOT,
    CCM_REQUIRE_ALLOWED_ROOTS: '1'
};
const ALLOWED_REDIRECT_HOSTS = new Set([
    'github.com',
    'objects.githubusercontent.com',
    'release-assets.githubusercontent.com'
]);

function positiveInteger(raw, fallback) {
    const value = Number(raw);
    return Number.isSafeInteger(value) && value > 0 ? value : fallback;
}

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

    installAgentSkill(home, path.join(__dirname, '..', 'SKILL.md'));

    if (installedCount === 0) {
        console.log("[CCM] No supported MCP config directories found.");
        console.log("[CCM] Add this server manually:");
        console.log(JSON.stringify({ [MCP_SERVER_NAME]: mcpConfig }, null, 2));
        console.log("[CCM] Codex example:");
        console.log(
            `codex mcp add ${MCP_SERVER_NAME}` +
                ` --env RUST_LOG=info` +
                ` --env CCM_PROJECT_ROOT=${INSTALL_PROJECT_ROOT}` +
                ` --env CCM_ALLOWED_ROOTS=${INSTALL_PROJECT_ROOT}` +
                ` --env CCM_REQUIRE_ALLOWED_ROOTS=1` +
                ` -- ${MCP_COMMAND} ${MCP_ARGS.join(' ')}`
        );
    } else {
        console.log("[CCM] Installation complete! Restart your AI editor to see the changes.");
    }
}

function installAgentSkill(home, sourcePath) {
    if (!fs.existsSync(sourcePath)) {
        throw new Error(`Packaged SKILL.md is missing: ${sourcePath}`);
    }

    const skillDirectory = path.join(home, '.agents', 'skills', MCP_SERVER_NAME);
    const skillPath = path.join(skillDirectory, 'SKILL.md');
    fs.mkdirSync(skillDirectory, { recursive: true });
    writeTextAtomic(skillPath, fs.readFileSync(sourcePath, 'utf8'));
    console.log('[CCM] ✓ Successfully updated: ~/.agents/skills/context-manager/SKILL.md');
}

function installJsonConfig(configPath, mcpConfig) {
    const dir = path.dirname(configPath);

    if (!fs.existsSync(dir)) {
        return false;
    }

    let config = { mcpServers: {} };
    if (fs.existsSync(configPath)) {
        const backupPath = `${configPath}.bak`;
        fs.copyFileSync(configPath, backupPath);
        try {
            const content = fs.readFileSync(configPath, 'utf8');
            config = JSON.parse(content);
        } catch (e) {
            throw new Error(
                `Could not parse ${configPath}. The original file was preserved and copied to ${backupPath}.`
            );
        }
    }

    if (!config.mcpServers || typeof config.mcpServers !== 'object') {
        config.mcpServers = {};
    }

    config.mcpServers[MCP_SERVER_NAME] = mcpConfig;
    writeJsonAtomic(configPath, config);
    console.log(`[CCM] ✓ Successfully updated: ${configPath}`);
    return true;
}

function writeJsonAtomic(configPath, value) {
    const tempPath = `${configPath}.${process.pid}.${Date.now()}.tmp`;
    try {
        fs.writeFileSync(tempPath, `${JSON.stringify(value, null, 2)}\n`, {
            encoding: 'utf8',
            mode: 0o600
        });
        fs.renameSync(tempPath, configPath);
    } catch (error) {
        if (fs.existsSync(tempPath)) {
            fs.unlinkSync(tempPath);
        }
        throw error;
    }
}

function installCodexConfig() {
    const codexDirectory = path.join(os.homedir(), '.codex');
    if (!fs.existsSync(codexDirectory)) {
        return false;
    }
    const configPath = path.join(codexDirectory, 'config.toml');
    installCodexTomlConfig(configPath, process.cwd(), VERSION);
    console.log('[CCM] ✓ Successfully updated: ~/.codex/config.toml');
    return true;
}

function installCodexTomlConfig(configPath, projectRoot, version) {
    let content = '';
    if (fs.existsSync(configPath)) {
        content = fs.readFileSync(configPath, 'utf8');
        fs.copyFileSync(configPath, `${configPath}.bak`);
    }

    const sectionPrefix = `mcp_servers.${MCP_SERVER_NAME}`;
    const lines = content.split(/\r?\n/);
    const preserved = [];
    let removing = false;
    for (const line of lines) {
        const header = line.trim().match(/^\[([^\]]+)\]$/);
        if (header) {
            removing = header[1] === sectionPrefix || header[1].startsWith(`${sectionPrefix}.`);
        }
        if (!removing) {
            preserved.push(line);
        }
    }

    const quote = (value) => JSON.stringify(String(value));
    const block = [
        `[${sectionPrefix}]`,
        `command = ${quote(MCP_COMMAND)}`,
        `args = [${['-y', `@senoldogann/context-manager@${version}`, 'mcp'].map(quote).join(', ')}]`,
        'enabled = true',
        '',
        `[${sectionPrefix}.env]`,
        `RUST_LOG = ${quote('info')}`,
        `CCM_PROJECT_ROOT = ${quote(projectRoot)}`,
        `CCM_ALLOWED_ROOTS = ${quote(projectRoot)}`,
        `CCM_REQUIRE_ALLOWED_ROOTS = ${quote('1')}`
    ].join('\n');

    const next = `${preserved.join('\n').trimEnd()}\n\n${block}\n`.replace(/^\n+/, '');
    writeTextAtomic(configPath, next);
}

function createUniqueTmpPath(binPath) {
    return `${binPath}.${process.pid}.${Date.now()}.tmp`;
}

function writeTextAtomic(filePath, content) {
    const tempPath = `${filePath}.${process.pid}.${Date.now()}.tmp`;
    try {
        fs.writeFileSync(tempPath, content, { encoding: 'utf8', mode: 0o600 });
        fs.renameSync(tempPath, filePath);
    } catch (error) {
        if (fs.existsSync(tempPath)) fs.unlinkSync(tempPath);
        throw error;
    }
}

async function getBinaryFor(commandName) {
    const platform = os.platform();
    const arch = os.arch();

    let target;
    if (platform === 'darwin') {
        if (arch === 'arm64') target = 'aarch64-apple-darwin';
        if (arch === 'x64') target = 'x86_64-apple-darwin';
    } else if (platform === 'linux') {
        if (arch === 'arm64') target = 'aarch64-unknown-linux-gnu';
        if (arch === 'x64') target = 'x86_64-unknown-linux-gnu';
    } else if (platform === 'win32') {
        if (arch === 'x64') target = 'x86_64-pc-windows-msvc.exe';
    }
    if (!target) {
        throw new Error(`Unsupported platform: ${platform}/${arch}`);
    }

    const binFilename = `${commandName}-v${VERSION}-${target}`;
    const binPath = path.join(BIN_DIR, binFilename);
    const remoteFilename = `${commandName}-${target}`;
    const compressedFilename = `${remoteFilename}.gz`;

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

    const tmpPath = createUniqueTmpPath(binPath);
    const compressedPath = `${binPath}.gz.download`;
    const rawPath = `${binPath}.download`;

    try {
        const compressedUrl =
            `https://github.com/${REPO}/releases/download/v${VERSION}/${compressedFilename}`;
        try {
            await downloadFileWithRetry(compressedUrl, compressedPath);
            await verifyChecksum(compressedPath, [compressedFilename]);
            await extractGzip(compressedPath, tmpPath);
            fs.unlinkSync(compressedPath);
        } catch (error) {
            if (error.statusCode !== 404) {
                throw error;
            }
            if (fs.existsSync(compressedPath)) fs.unlinkSync(compressedPath);
            const rawUrl =
                `https://github.com/${REPO}/releases/download/v${VERSION}/${remoteFilename}`;
            await downloadFileWithRetry(rawUrl, rawPath);
            await verifyChecksum(rawPath, [remoteFilename, binFilename]);
            fs.renameSync(rawPath, tmpPath);
        }
        fs.chmodSync(tmpPath, '755');
        if (fs.existsSync(binPath)) {
            fs.unlinkSync(tmpPath);
            return binPath;
        }
        fs.renameSync(tmpPath, binPath);
    } catch (err) {
        if (fs.existsSync(tmpPath)) fs.unlinkSync(tmpPath);
        if (/checksum|gzip|header|unexpected end/i.test(err.message || '')) {
            if (fs.existsSync(compressedPath)) fs.unlinkSync(compressedPath);
            if (fs.existsSync(rawPath)) fs.unlinkSync(rawPath);
        }
        throw err;
    }

    return binPath;
}

async function extractGzip(source, destination) {
    await pipeline(
        fs.createReadStream(source),
        zlib.createGunzip(),
        fs.createWriteStream(destination)
    );
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

function downloadFile(url, dest, redirectsRemaining = 5) {
    return new Promise((resolve, reject) => {
        fs.mkdirSync(path.dirname(dest), { recursive: true });
        const existingBytes = fs.existsSync(dest) ? fs.statSync(dest).size : 0;
        const headers = existingBytes > 0 ? { Range: `bytes=${existingBytes}-` } : {};
        const request = https.get(url, { headers }, (response) => {
            if (response.statusCode === 302 || response.statusCode === 301) {
                response.resume();
                if (redirectsRemaining <= 0) {
                    return reject(new Error('Too many redirects while downloading binary'));
                }
                try {
                    const redirectUrl = resolveRedirectUrl(url, response.headers.location);
                    return downloadFile(redirectUrl, dest, redirectsRemaining - 1)
                        .then(resolve)
                        .catch(reject);
                } catch (error) {
                    return reject(error);
                }
            }
            if (response.statusCode === 416 && existingBytes > 0) {
                response.resume();
                return resolve();
            }
            if (response.statusCode !== 200 && response.statusCode !== 206) {
                response.resume();
                const error = new Error(`Failed to download: ${response.statusCode}`);
                error.statusCode = response.statusCode;
                return reject(error);
            }
            const append = response.statusCode === 206 && existingBytes > 0;
            const file = fs.createWriteStream(dest, { flags: append ? 'a' : 'w' });
            pipeline(response, file).then(resolve).catch(reject);
        });
        request.setTimeout(DOWNLOAD_TIMEOUT_MS, () => {
            request.destroy(new Error(`Download timed out after ${DOWNLOAD_TIMEOUT_MS}ms`));
        });
        request.on('error', (err) => {
            reject(err);
        });
    });
}

async function downloadFileWithRetry(url, dest, attempts = DOWNLOAD_ATTEMPTS) {
    let lastError;
    for (let attempt = 1; attempt <= attempts; attempt++) {
        try {
            await downloadFile(url, dest);
            return;
        } catch (error) {
            lastError = error;
            if (error.statusCode === 404 || attempt === attempts) break;
            console.warn(`[CCM] Download interrupted; retrying (${attempt}/${attempts})...`);
            await new Promise((resolve) => setTimeout(resolve, attempt * 500));
        }
    }
    throw lastError;
}

function downloadText(url, redirectsRemaining = 5) {
    return new Promise((resolve, reject) => {
        const request = https.get(url, (response) => {
            if (response.statusCode === 302 || response.statusCode === 301) {
                response.resume();
                if (redirectsRemaining <= 0) {
                    return reject(new Error('Too many redirects while downloading checksums'));
                }
                try {
                    const redirectUrl = resolveRedirectUrl(url, response.headers.location);
                    return downloadText(redirectUrl, redirectsRemaining - 1)
                        .then(resolve)
                        .catch(reject);
                } catch (error) {
                    return reject(error);
                }
            }
            if (response.statusCode !== 200) {
                response.resume();
                const error = new Error(`Failed to download: ${response.statusCode}`);
                error.statusCode = response.statusCode;
                return reject(error);
            }
            let data = '';
            response.setEncoding('utf8');
            response.on('data', (chunk) => {
                data += chunk;
            });
            response.on('end', () => resolve(data));
        });
        request.setTimeout(DOWNLOAD_TIMEOUT_MS, () => {
            request.destroy(new Error(`Download timed out after ${DOWNLOAD_TIMEOUT_MS}ms`));
        });
        request.on('error', reject);
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

if (require.main === module) {
    main();
}

module.exports = {
    MCP_ARGS,
    MCP_ENV,
    extractGzip,
    installAgentSkill,
    installCodexTomlConfig,
    installJsonConfig,
    parseChecksums,
    resolveRedirectUrl,
    verifyChecksum,
    writeJsonAtomic
};

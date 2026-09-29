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
// Kurulum dizini allowlist'e eklenir ve host proje bildirmezse varsayılan kök
// olur. CCM_PROJECT_ROOT yazılmaz: açık kök, istemcinin MCP roots ile bildirdiği
// çalışma alanını her zaman ezer ve başka projelerde yanlış indeksi döndürür.
const INSTALL_PROJECT_ROOT = process.cwd();
const MCP_ENV = {
    RUST_LOG: 'info',
    CCM_ALLOWED_ROOTS: INSTALL_PROJECT_ROOT,
    CCM_REQUIRE_ALLOWED_ROOTS: '1'
};
// Claude Code sunucuyu proje dizininde başlatır; non-strict mod yalnızca bu
// başlatma dizinini (ve MCP roots'u) izinli kılar, sabit bir kök gerekmez.
const CLAUDE_CODE_ENV = {
    RUST_LOG: 'info',
    CCM_REQUIRE_ALLOWED_ROOTS: '0'
};
// Kurucunun sahip olduğu env anahtarları; kullanıcının eklediği diğer anahtarlar
// (ör. EMBEDDING_* ayarları) yeniden kurulumda korunur.
const MANAGED_ENV_KEYS = new Set([
    'RUST_LOG',
    'CCM_PROJECT_ROOT',
    'CCM_ALLOWED_ROOTS',
    'CCM_REQUIRE_ALLOWED_ROOTS'
]);
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

    // Her hedef bağımsızdır: bozuk bir config diğer hostların kurulumunu durdurmaz;
    // hatalar sonda toplanıp komut başarısız olarak sonlanır.
    const installers = [
        ...jsonTargets.map((configPath) => ({
            name: configPath,
            run: () => installJsonConfig(configPath, mcpConfig)
        })),
        { name: '~/.codex/config.toml', run: () => installCodexConfig() },
        {
            name: 'Claude Code (user scope)',
            run: () => installClaudeCodeConfig(runClaudeCli, path.join(home, '.claude.json'))
        }
    ];
    let installedCount = 0;
    const failures = [];
    for (const installer of installers) {
        try {
            if (installer.run()) {
                installedCount++;
            }
        } catch (error) {
            failures.push({ name: installer.name, message: error.message });
        }
    }

    installAgentSkill(path.join(home, '.agents', 'skills'), path.join(__dirname, '..', 'SKILL.md'));
    if (fs.existsSync(path.join(home, '.claude'))) {
        installAgentSkill(path.join(home, '.claude', 'skills'), path.join(__dirname, '..', 'SKILL.md'));
    }

    if (failures.length > 0) {
        for (const failure of failures) {
            console.error(`[CCM] ✗ ${failure.name}: ${failure.message}`);
        }
        throw new Error(`${failures.length} MCP configuration target(s) failed; see messages above.`);
    }

    if (installedCount === 0) {
        console.log("[CCM] No supported MCP config directories found.");
        console.log("[CCM] Add this server manually:");
        console.log(JSON.stringify({ [MCP_SERVER_NAME]: mcpConfig }, null, 2));
        console.log("[CCM] Codex example:");
        console.log(
            `codex mcp add ${MCP_SERVER_NAME}` +
                ` --env RUST_LOG=info` +
                ` --env CCM_ALLOWED_ROOTS=${INSTALL_PROJECT_ROOT}` +
                ` --env CCM_REQUIRE_ALLOWED_ROOTS=1` +
                ` -- ${MCP_COMMAND} ${MCP_ARGS.join(' ')}`
        );
    } else {
        console.log("[CCM] Installation complete! Restart your AI editor to see the changes.");
        console.log(
            `[CCM] Hosts that do not report their workspace default to ${INSTALL_PROJECT_ROOT}. ` +
                'Extend CCM_ALLOWED_ROOTS in the host config to allow more projects.'
        );
    }
}

function installAgentSkill(skillsRoot, sourcePath) {
    if (!fs.existsSync(sourcePath)) {
        throw new Error(`Packaged SKILL.md is missing: ${sourcePath}`);
    }

    const skillDirectory = path.join(skillsRoot, MCP_SERVER_NAME);
    const skillPath = path.join(skillDirectory, 'SKILL.md');
    fs.mkdirSync(skillDirectory, { recursive: true });
    const nextContent = fs.readFileSync(sourcePath, 'utf8');
    if (fs.existsSync(skillPath) && fs.readFileSync(skillPath, 'utf8') !== nextContent) {
        const backupPath = backupFileOnce(skillPath);
        console.warn(`[CCM] Existing agent skill was backed up to ${backupPath}`);
    }
    writeTextAtomic(skillPath, nextContent);
    console.log(`[CCM] ✓ Successfully updated: ${skillPath}`);
}

/**
 * Dosyanın mevcut içeriğini yedekler ve yedek yolunu döndürür. İlk yedek `.bak`
 * olur; sonraki farklı içerikler `.bak.<hash>` ile saklanır, hiçbir yedeğin
 * üzerine yazılmaz (ilk orijinal config her zaman geri alınabilir kalır).
 */
function backupFileOnce(filePath) {
    const content = fs.readFileSync(filePath);
    const primaryBackup = `${filePath}.bak`;
    if (!fs.existsSync(primaryBackup)) {
        fs.copyFileSync(filePath, primaryBackup, fs.constants.COPYFILE_EXCL);
        return primaryBackup;
    }
    if (fs.readFileSync(primaryBackup).equals(content)) {
        return primaryBackup;
    }
    const contentHash = crypto.createHash('sha256').update(content).digest('hex').slice(0, 16);
    const hashedBackup = `${primaryBackup}.${contentHash}`;
    if (!fs.existsSync(hashedBackup)) {
        fs.copyFileSync(filePath, hashedBackup, fs.constants.COPYFILE_EXCL);
    }
    return hashedBackup;
}

/** Kurucunun yönettiği anahtarları atıp kullanıcının env anahtarlarını korur. */
function mergeManagedEnv(existingEnv, managedEnv) {
    const preserved = Object.fromEntries(
        Object.entries(existingEnv).filter(([key]) => !MANAGED_ENV_KEYS.has(key))
    );
    return { ...preserved, ...managedEnv };
}

function installJsonConfig(configPath, mcpConfig) {
    const dir = path.dirname(configPath);

    if (!fs.existsSync(dir)) {
        return false;
    }

    let config = { mcpServers: {} };
    if (fs.existsSync(configPath)) {
        const backupPath = backupFileOnce(configPath);
        try {
            const content = fs.readFileSync(configPath, 'utf8');
            config = JSON.parse(content);
        } catch (e) {
            throw new Error(
                `Could not parse ${configPath}. The original file was preserved and copied to ${backupPath}.`
            );
        }
    }

    if (config === null || Array.isArray(config) || typeof config !== 'object') {
        throw new Error(
            `Could not update ${configPath}: the top-level JSON value must be an object. The original file was preserved.`
        );
    }

    if (!config.mcpServers) {
        config.mcpServers = {};
    } else if (Array.isArray(config.mcpServers) || typeof config.mcpServers !== 'object') {
        throw new Error(
            `Could not update ${configPath}: mcpServers must be a JSON object. The original file was preserved.`
        );
    }

    const existingEnv = config.mcpServers[MCP_SERVER_NAME]?.env ?? {};
    config.mcpServers[MCP_SERVER_NAME] = {
        ...mcpConfig,
        env: mergeManagedEnv(existingEnv, mcpConfig.env)
    };
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

/** `claude` CLI'ını çalıştırır; CLI yoksa `null` döner. */
function runClaudeCli(args) {
    const result = require('child_process').spawnSync('claude', args, {
        encoding: 'utf8',
        shell: os.platform() === 'win32'
    });
    if (result.error && result.error.code === 'ENOENT') {
        return null;
    }
    if (result.error) {
        throw result.error;
    }
    return { status: result.status, stdout: result.stdout, stderr: result.stderr };
}

/**
 * Claude Code'a kullanıcı kapsamında kaydeder. `~/.claude.json` doğrudan
 * yazılmaz (çalışan Claude Code ile yarışır); resmi `claude mcp` CLI'ı kullanılır.
 * Mevcut kayıttaki kullanıcı env anahtarları okunup korunur.
 */
function installClaudeCodeConfig(runClaude, claudeConfigPath) {
    if (runClaude(['--version']) === null) {
        return false;
    }
    let existingEnv = {};
    if (fs.existsSync(claudeConfigPath)) {
        try {
            const claudeConfig = JSON.parse(fs.readFileSync(claudeConfigPath, 'utf8'));
            existingEnv = claudeConfig?.mcpServers?.[MCP_SERVER_NAME]?.env ?? {};
        } catch (error) {
            throw new Error(`Could not parse ${claudeConfigPath} to preserve existing env: ${error.message}`);
        }
    }

    const removal = runClaude(['mcp', 'remove', MCP_SERVER_NAME, '--scope', 'user']);
    const removalOutput = `${removal.stdout}${removal.stderr}`;
    if (removal.status !== 0 && !/no .*mcp server|not found/i.test(removalOutput)) {
        throw new Error(`claude mcp remove failed (exit ${removal.status}): ${removalOutput.trim()}`);
    }

    const serverJson = JSON.stringify({
        type: 'stdio',
        command: MCP_COMMAND,
        args: MCP_ARGS,
        env: mergeManagedEnv(existingEnv, CLAUDE_CODE_ENV)
    });
    const added = runClaude(['mcp', 'add-json', MCP_SERVER_NAME, serverJson, '--scope', 'user']);
    if (added.status !== 0) {
        throw new Error(`claude mcp add-json failed (exit ${added.status}): ${`${added.stdout}${added.stderr}`.trim()}`);
    }
    console.log('[CCM] ✓ Successfully updated: Claude Code (user scope)');
    return true;
}

function installCodexTomlConfig(configPath, projectRoot, version) {
    let content = '';
    if (fs.existsSync(configPath)) {
        content = fs.readFileSync(configPath, 'utf8');
        backupFileOnce(configPath);
    }

    const sectionPrefix = `mcp_servers.${MCP_SERVER_NAME}`;
    const lines = content.split(/\r?\n/);
    const preserved = [];
    // Kullanıcının env tablosuna eklediği satırlar (yönetilen anahtarlar hariç) korunur.
    const preservedEnvLines = [];
    let removing = false;
    let inEnvSection = false;
    for (const line of lines) {
        const header = line.match(/^\s*\[\[?([^\]]+)\]\]?\s*(?:#.*)?$/);
        if (header) {
            removing = isManagedCodexSection(header[1]);
            inEnvSection = removing && /\.\s*env\s*$/.test(header[1].trim());
        } else if (inEnvSection) {
            const key = line.match(/^\s*("?)([A-Za-z0-9_]+)\1\s*=/);
            if (key && !MANAGED_ENV_KEYS.has(key[2])) {
                preservedEnvLines.push(line.trim());
            }
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
        `CCM_ALLOWED_ROOTS = ${quote(projectRoot)}`,
        `CCM_REQUIRE_ALLOWED_ROOTS = ${quote('1')}`,
        ...preservedEnvLines
    ].join('\n');

    const next = `${preserved.join('\n').trimEnd()}\n\n${block}\n`.replace(/^\n+/, '');
    validateManagedCodexSections(next);
    writeTextAtomic(configPath, next);
    try {
        const written = fs.readFileSync(configPath, 'utf8');
        if (written !== next) {
            throw new Error('Codex configuration verification did not match the rendered content');
        }
        validateManagedCodexSections(written);
    } catch (error) {
        if (content === '') {
            fs.unlinkSync(configPath);
        } else {
            writeTextAtomic(configPath, content);
        }
        throw error;
    }
}

function isManagedCodexSection(header) {
    return /^(?:mcp_servers|"mcp_servers"|'mcp_servers')\s*\.\s*(?:context-manager|"context-manager"|'context-manager')\s*(?:\.|$)/.test(
        header.trim()
    );
}

function validateManagedCodexSections(content) {
    let rootCount = 0;
    let envCount = 0;
    for (const line of content.split(/\r?\n/)) {
        const header = line.trim().match(/^\[([^\]]+)\]$/);
        if (!header || !isManagedCodexSection(header[1])) continue;
        const normalized = header[1]
            .replace(/"context-manager"|'context-manager'/, 'context-manager')
            .replace(/\s+/g, '');
        if (normalized === 'mcp_servers.context-manager') rootCount++;
        if (normalized === 'mcp_servers.context-manager.env') envCount++;
    }
    if (rootCount !== 1 || envCount !== 1) {
        throw new Error(
            `Rendered Codex configuration is invalid: expected one context-manager root and env section, found root=${rootCount}, env=${envCount}`
        );
    }
}

function createUniqueTmpPath(binPath) {
    const nonce = crypto.randomBytes(8).toString('hex');
    return `${binPath}.${process.pid}.${Date.now()}.${nonce}.tmp`;
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

    // Daha once dogrulanmis cache yalniz sidecar hash'i halen eslesiyorsa kullanilir.
    if (fs.existsSync(binPath)) {
        if (await verifyCachedBinary(binPath)) {
            fs.chmodSync(binPath, '755');
            return binPath;
        }
        console.warn(`[CCM] Cached binary failed verification and will be replaced: ${binPath}`);
    }

    // stderr: `mcp` modunda stdout yalnızca JSON-RPC kanalına aittir.
    console.error(`[CCM] Downloading ${commandName} v${VERSION} for ${target}...`);

    if (!fs.existsSync(BIN_DIR)) {
        fs.mkdirSync(BIN_DIR, { recursive: true });
    }

    const tmpPath = createUniqueTmpPath(binPath);
    const compressedPath = `${tmpPath}.gz.download`;
    const rawPath = `${tmpPath}.download`;

    try {
        let downloadVerified = false;
        const compressedUrl =
            `https://github.com/${REPO}/releases/download/v${VERSION}/${compressedFilename}`;
        try {
            await downloadFileWithRetry(compressedUrl, compressedPath);
            downloadVerified = await verifyChecksum(compressedPath, [compressedFilename]);
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
            downloadVerified = await verifyChecksum(rawPath, [remoteFilename, binFilename]);
            fs.renameSync(rawPath, tmpPath);
        }
        fs.chmodSync(tmpPath, '755');
        await finalizeDownloadedBinary(binPath, tmpPath, downloadVerified);
    } catch (err) {
        if (fs.existsSync(tmpPath)) fs.unlinkSync(tmpPath);
        // Retry dongusu ayni unique partial dosyayi kullanir. Son hata sonrasinda
        // baska bir proses bu yolu yeniden kullanamayacagi icin artiklari temizle.
        if (fs.existsSync(compressedPath)) fs.unlinkSync(compressedPath);
        if (fs.existsSync(rawPath)) fs.unlinkSync(rawPath);
        throw err;
    }

    return binPath;
}

function processIsAlive(pid) {
    if (!Number.isInteger(pid) || pid <= 0) return false;
    try {
        process.kill(pid, 0);
        return true;
    } catch (error) {
        return error.code === 'EPERM';
    }
}

async function acquireFileLock(lockPath, timeoutMs) {
    const startedAt = Date.now();
    while (true) {
        try {
            const descriptor = fs.openSync(lockPath, 'wx', 0o600);
            fs.writeFileSync(descriptor, `${process.pid}\n`, 'utf8');
            return descriptor;
        } catch (error) {
            if (error.code !== 'EEXIST') throw error;
            let ownerAlive = true;
            try {
                const owner = Number.parseInt(fs.readFileSync(lockPath, 'utf8').trim(), 10);
                ownerAlive = processIsAlive(owner);
            } catch (readError) {
                if (readError.code !== 'ENOENT') ownerAlive = false;
            }
            if (!ownerAlive) {
                try {
                    fs.unlinkSync(lockPath);
                } catch (unlinkError) {
                    if (unlinkError.code !== 'ENOENT') throw unlinkError;
                }
                continue;
            }
            if (Date.now() - startedAt >= timeoutMs) {
                throw new Error(`Timed out waiting for binary cache lock: ${lockPath}`);
            }
            await new Promise((resolve) => setTimeout(resolve, 50));
        }
    }
}

async function finalizeDownloadedBinary(binPath, tmpPath, downloadVerified) {
    const lockPath = `${binPath}.lock`;
    const descriptor = await acquireFileLock(lockPath, 30_000);
    try {
        const downloadedHash = await sha256File(tmpPath);
        if (fs.existsSync(binPath)) {
            const existingHash = await sha256File(binPath);
            if (existingHash === downloadedHash) {
                fs.unlinkSync(tmpPath);
                if (downloadVerified) writeBinaryChecksumSidecar(binPath, existingHash);
                return;
            }
            fs.unlinkSync(binPath);
            const staleSidecar = checksumSidecarPath(binPath);
            if (fs.existsSync(staleSidecar)) fs.unlinkSync(staleSidecar);
        }
        fs.renameSync(tmpPath, binPath);
        if (downloadVerified) writeBinaryChecksumSidecar(binPath, downloadedHash);
    } finally {
        fs.closeSync(descriptor);
        try {
            fs.unlinkSync(lockPath);
        } catch (error) {
            if (error.code !== 'ENOENT') throw error;
        }
    }
}

function checksumSidecarPath(binPath) {
    return `${binPath}.sha256`;
}

function writeBinaryChecksumSidecar(binPath, hash) {
    writeTextAtomic(checksumSidecarPath(binPath), `${hash}\n`);
}

async function verifyCachedBinary(binPath) {
    const sidecarPath = checksumSidecarPath(binPath);
    if (!fs.existsSync(sidecarPath)) return false;
    const expected = fs.readFileSync(sidecarPath, 'utf8').trim().toLowerCase();
    if (!/^[a-f0-9]{64}$/.test(expected)) return false;
    const actual = await sha256File(binPath);
    return actual === expected;
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
            return false;
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
            return false;
        }
        throw new Error('Checksum not found for binary. Set CCM_ALLOW_UNVERIFIED_BINARIES=1 to bypass.');
    }

    const actual = await sha256File(filePath);
    if (actual !== expected) {
        throw new Error(`Checksum mismatch. Expected ${expected}, got ${actual}`);
    }
    return true;
}

async function main() {
    try {
        const binPath = await getBinary();
        const args = process.argv.slice(2);

        // `mcp` için kök enjekte edilmez: açık CCM_PROJECT_ROOT istemcinin MCP
        // roots ile bildirdiği çalışma alanını ezer. CLI komutları cwd'yi kullanır.
        const env = args[0] === 'mcp'
            ? process.env
            : { ...process.env, CCM_PROJECT_ROOT: process.env.CCM_PROJECT_ROOT || process.cwd() };
        const child = spawn(binPath, args, { stdio: 'inherit', env });

        child.on('error', (error) => {
            console.error(`[CCM Error] Failed to start binary: ${error.message}`);
            process.exit(1);
        });
        child.on('exit', (code, signal) => {
            if (signal) {
                console.error(`[CCM Error] Binary terminated by signal ${signal}`);
                process.exit(1);
            }
            process.exit(typeof code === 'number' ? code : 1);
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
    backupFileOnce,
    createUniqueTmpPath,
    extractGzip,
    finalizeDownloadedBinary,
    installAgentSkill,
    installClaudeCodeConfig,
    installCodexTomlConfig,
    installJsonConfig,
    parseChecksums,
    resolveRedirectUrl,
    verifyCachedBinary,
    verifyChecksum,
    writeJsonAtomic
};

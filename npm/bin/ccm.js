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

const LOCK_OWNER_FILE = 'owner';

/**
 * Kilit dizini içindeki sahip pid dosyasının yolunu döndürür.
 */
function lockOwnerPath(lockPath) {
    return path.join(lockPath, LOCK_OWNER_FILE);
}

/**
 * Kilidi pid içeriğiyle birlikte tek adımda oluşturur: önce benzersiz bir geçici
 * dizin açılır, pid onun içindeki dosyaya yazılır, sonra dizin `renameSync` ile
 * kilit yoluna taşınır. Dizin taşıma hard link gerektirmez (exFAT, ağ diskleri)
 * ve atomiktir; kilit hiçbir zaman pid'siz görünmez. Kilit doluysa taşıma
 * başarısız olur ve özgün hata olduğu gibi fırlatılır.
 */
function createPidLockAtomic(lockPath) {
    const tempDir = createUniqueTmpPath(lockPath);
    fs.mkdirSync(tempDir, { mode: 0o700 });
    try {
        fs.writeFileSync(lockOwnerPath(tempDir), `${process.pid}\n`, {
            encoding: 'utf8',
            mode: 0o600,
            flag: 'wx'
        });
        fs.renameSync(tempDir, lockPath);
    } catch (error) {
        fs.rmSync(tempDir, { recursive: true, force: true });
        throw error;
    }
}

/**
 * Kilit dizinini oluşturma hatasının "kilit başka bir proseste" anlamına gelip
 * gelmediğine karar verir. POSIX dolu hedef dizin için EEXIST/ENOTEMPTY verir;
 * Windows ve bazı ağ dosya sistemleri EPERM/EACCES verir, bu durumda hedefin
 * gerçekten var olması şart koşulur.
 */
function isLockHeldError(errorCode, lockExists) {
    if (errorCode === 'EEXIST' || errorCode === 'ENOTEMPTY') return true;
    return (errorCode === 'EPERM' || errorCode === 'EACCES') && lockExists;
}

/**
 * Kilidi atomik olarak kaldırır: dizin önce benzersiz bir çöp yoluna taşınır,
 * sonra silinir. Böylece yarım silinmiş (boş) bir kilit dizini hiçbir zaman
 * kilit yolunda kalmaz. Kilit zaten yoksa `false` döner.
 */
function removeLockAtomic(lockPath) {
    const trashPath = createUniqueTmpPath(lockPath);
    try {
        fs.renameSync(lockPath, trashPath);
    } catch (error) {
        if (error.code === 'ENOENT') return false;
        throw error;
    }
    fs.rmSync(trashPath, { recursive: true, force: false });
    return true;
}

/**
 * Kilit sahibinin hâlâ yaşayıp yaşamadığını okur. Kilit bu arada bırakıldıysa
 * (ENOENT) sahip canlı sayılır ve döngü yeniden dener; okunamayan veya bozuk
 * pid ölü sayılır, böylece bayat kilit devralınabilir.
 */
function lockOwnerIsAlive(lockPath) {
    try {
        const owner = Number.parseInt(fs.readFileSync(lockOwnerPath(lockPath), 'utf8').trim(), 10);
        return processIsAlive(owner);
    } catch (readError) {
        return readError.code === 'ENOENT';
    }
}

async function acquireFileLock(lockPath, timeoutMs) {
    const startedAt = Date.now();
    while (true) {
        try {
            createPidLockAtomic(lockPath);
            return;
        } catch (error) {
            if (!isLockHeldError(error.code, fs.existsSync(lockPath))) throw error;
            if (!lockOwnerIsAlive(lockPath)) {
                removeLockAtomic(lockPath);
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
    const lockPath = binaryLockPath(binPath);
    await acquireFileLock(lockPath, 30_000);
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
        removeLockAtomic(lockPath);
    }
}

/**
 * İkili önbellek kilidinin dizin yolu. Eski sürümlerin `.lock` dosyasıyla
 * çakışmasın diye ayrı bir ad kullanılır.
 */
function binaryLockPath(binPath) {
    return `${binPath}.lockdir`;
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

/**
 * Tam `process.argv` dizisinden hangi aracın çalışacağını çözer; girdiyi
 * değiştirmez. Dönüş etiketlidir:
 * - `{ kind: 'mcp', command: 'ccm-mcp', args }`: `ccm-mcp` bin adıyla ya da
 *   `npx … mcp` ile çağrıldı (`mcp` alt komutu argümanlardan çıkarılır),
 * - `{ kind: 'install' }`: `install` alt komutu,
 * - `{ kind: 'cli', command: 'ccm-cli', args }`: diğer her şey.
 */
function parseLaunch(argv) {
    if (typeof argv[1] !== 'string' || argv[1] === '') {
        throw new Error(`Cannot determine the invoked script from argv: ${JSON.stringify(argv)}`);
    }
    const binName = path.basename(argv[1]);
    const userArgs = argv.slice(2);

    if (binName.includes('mcp')) {
        return { kind: 'mcp', command: 'ccm-mcp', args: userArgs };
    }
    if (userArgs[0] === 'mcp') {
        return { kind: 'mcp', command: 'ccm-mcp', args: userArgs.slice(1) };
    }
    if (userArgs[0] === 'install') {
        return { kind: 'install' };
    }
    return { kind: 'cli', command: 'ccm-cli', args: userArgs };
}

/**
 * CLI komutları için env: kullanıcı CCM_PROJECT_ROOT vermediyse cwd kök olur.
 * MCP modunda bu fonksiyon kullanılmaz; açık kök istemcinin MCP roots ile
 * bildirdiği çalışma alanını ezer (bkz. sunucudaki project_root_is_explicit).
 */
function cliChildEnv(env, cwd) {
    return { ...env, CCM_PROJECT_ROOT: env.CCM_PROJECT_ROOT || cwd };
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

/**
 * İndirmeyi artan bekleme ile yeniden dener; her denemede uyarı yazar ve son
 * hatayı fırlatır. 404 kalıcıdır: yeniden denenmez, hemen fırlatılır.
 */
async function retryDownloadUnlessNotFound(url, attempts, download) {
    let lastError;
    for (let attempt = 1; attempt <= attempts; attempt++) {
        try {
            return await download();
        } catch (error) {
            lastError = error;
            if (error.statusCode === 404 || attempt === attempts) break;
            console.warn(
                `[CCM] Download interrupted; retrying (${attempt}/${attempts})...`,
                { url, error: error.message }
            );
            await new Promise((resolve) => setTimeout(resolve, attempt * 500));
        }
    }
    throw lastError;
}

async function downloadFileWithRetry(url, dest, attempts = DOWNLOAD_ATTEMPTS) {
    await retryDownloadUnlessNotFound(url, attempts, () => downloadFile(url, dest));
}

function downloadTextWithRetry(url, attempts) {
    return retryDownloadUnlessNotFound(url, attempts, () => downloadText(url));
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

/**
 * Sürümün checksum manifestinin durumunu döndürür:
 * - `{ kind: 'available', checksums }`: manifest indirildi ve geçerli,
 * - `{ kind: 'absent' }`: sürümde manifest yok (404),
 * - `{ kind: 'fetch-failed', url, error }`: ağ hatası (yeniden denemelerden
 *   sonra) ya da bozuk manifest. Geçici olabileceği için önbelleğe alınmaz.
 */
async function getChecksums() {
    if (checksumCache) return checksumCache;

    const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${CHECKSUMS_FILE}`;
    let text;
    try {
        text = await downloadTextWithRetry(url, DOWNLOAD_ATTEMPTS);
    } catch (error) {
        return rememberChecksumManifest(classifyManifestError(url, error));
    }
    return rememberChecksumManifest(classifyManifestText(url, text));
}

function rememberChecksumManifest(manifest) {
    if (manifest.kind !== 'fetch-failed') checksumCache = manifest;
    return manifest;
}

/** İndirme hatasını sınıflandırır: yalnız 404 "manifest yok" anlamına gelir. */
function classifyManifestError(url, error) {
    if (error.statusCode === 404) return { kind: 'absent' };
    return { kind: 'fetch-failed', url, error };
}

/**
 * İndirilen manifest metnini sınıflandırır. Hiç girdisi olmayan ya da geçersiz
 * SHA-256 değeri içeren manifest bozuktur ve 'fetch-failed' sayılır; "manifest
 * yok" ile karıştırılıp doğrulamasız kuruluma izin vermez.
 */
function classifyManifestText(url, text) {
    const checksums = parseChecksums(text);
    if (checksums.size === 0) {
        return { kind: 'fetch-failed', url, error: new Error('Checksum manifest contains no entries') };
    }
    const malformed = [...checksums]
        .filter(([, hash]) => !/^[a-f0-9]{64}$/.test(hash))
        .map(([filename]) => filename);
    if (malformed.length > 0) {
        return {
            kind: 'fetch-failed',
            url,
            error: new Error(`Checksum manifest has invalid SHA-256 values for: ${malformed.join(', ')}`)
        };
    }
    return { kind: 'available', checksums };
}

/**
 * Manifest durumundan ikilinin beklenen hash'ini çözer:
 * - `{ kind: 'verified', hash }`: beklenen hash bulundu,
 * - `{ kind: 'absent', detail }`: sürümde bu ikili için checksum yok,
 * - `{ kind: 'fetch-failed', url, error }`: manifest alınamadı; asla atlanamaz.
 */
function resolveExpectedChecksum(manifest, candidates) {
    switch (manifest.kind) {
        case 'available': {
            const names = candidates.flatMap((name) => [name, `${name}.exe`]);
            const hash = names.map((name) => manifest.checksums.get(name)).find(Boolean);
            if (hash) return { kind: 'verified', hash };
            return { kind: 'absent', detail: `Checksum not found for binary (looked up: ${names.join(', ')})` };
        }
        case 'absent':
            return { kind: 'absent', detail: 'Checksum manifest not found' };
        case 'fetch-failed':
            return manifest;
        default:
            throw new Error(`Unhandled checksum manifest kind: ${manifest.kind}`);
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

/**
 * İndirilen dosyayı sürüm manifestine göre doğrular; doğrulandıysa `true`,
 * kullanıcının izniyle doğrulamasız geçildiyse `false` döner. İzin yalnız
 * sürümde checksum gerçekten yoksa geçerlidir; manifest alınamazsa her zaman
 * hata fırlatılır (geçici ağ hatası doğrulamasız kurulum sebebi değildir).
 */
async function verifyChecksum(filePath, candidates) {
    const expected = resolveExpectedChecksum(await getChecksums(), candidates);
    switch (expected.kind) {
        case 'verified': {
            const actual = await sha256File(filePath);
            if (actual !== expected.hash) {
                throw new Error(`Checksum mismatch for ${filePath}. Expected ${expected.hash}, got ${actual}`);
            }
            return true;
        }
        case 'absent':
            if (allowUnverifiedBinaries()) {
                console.warn(`[CCM] ${expected.detail}. Proceeding without verification.`);
                return false;
            }
            throw new Error(`${expected.detail}. Set CCM_ALLOW_UNVERIFIED_BINARIES=1 to bypass.`);
        case 'fetch-failed':
            throw new Error(
                `Could not obtain the checksum manifest ${expected.url}: ${expected.error.message}. ` +
                    'The binary was not installed; CCM_ALLOW_UNVERIFIED_BINARIES only bypasses releases ' +
                    'that publish no checksum manifest.',
                { cause: expected.error }
            );
        default:
            throw new Error(`Unhandled expected checksum kind: ${expected.kind}`);
    }
}

/** İkiliyi stdio'yu devrederek başlatır ve çıkış kodunu sürecin çıkışına taşır. */
function runBinary(binPath, args, env) {
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
}

async function main() {
    try {
        const launch = parseLaunch(process.argv);
        switch (launch.kind) {
            case 'install':
                await installMcp();
                process.exit(0);
                return;
            case 'mcp':
                // Kök enjekte edilmez; kullanıcının verdiği CCM_PROJECT_ROOT olduğu gibi kalır.
                runBinary(await getBinaryFor(launch.command), launch.args, process.env);
                return;
            case 'cli':
                runBinary(
                    await getBinaryFor(launch.command),
                    launch.args,
                    cliChildEnv(process.env, process.cwd())
                );
                return;
            default:
                throw new Error(`Unhandled launch kind: ${launch.kind}`);
        }
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
    binaryLockPath,
    classifyManifestError,
    classifyManifestText,
    cliChildEnv,
    createUniqueTmpPath,
    extractGzip,
    finalizeDownloadedBinary,
    installAgentSkill,
    installClaudeCodeConfig,
    installCodexTomlConfig,
    installJsonConfig,
    parseChecksums,
    parseLaunch,
    resolveExpectedChecksum,
    resolveRedirectUrl,
    verifyCachedBinary,
    verifyChecksum,
    writeJsonAtomic
};

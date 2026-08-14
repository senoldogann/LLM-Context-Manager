const assert = require('node:assert/strict');
const fs = require('node:fs');
const crypto = require('node:crypto');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const zlib = require('node:zlib');
const { execFileSync, spawnSync } = require('node:child_process');

const {
    MCP_ARGS,
    MCP_ENV,
    createUniqueTmpPath,
    extractGzip,
    finalizeDownloadedBinary,
    installAgentSkill,
    installCodexTomlConfig,
    installJsonConfig,
    verifyCachedBinary,
    writeJsonAtomic
} = require('../bin/ccm.js');

const packageRoot = path.resolve(__dirname, '..');
const wrapperPath = path.join(packageRoot, 'bin', 'ccm.js');

function releaseTarget() {
    if (process.platform === 'darwin' && process.arch === 'arm64') return 'aarch64-apple-darwin';
    if (process.platform === 'darwin' && process.arch === 'x64') return 'x86_64-apple-darwin';
    if (process.platform === 'linux' && process.arch === 'arm64') return 'aarch64-unknown-linux-gnu';
    if (process.platform === 'linux' && process.arch === 'x64') return 'x86_64-unknown-linux-gnu';
    if (process.platform === 'win32' && process.arch === 'x64') return 'x86_64-pc-windows-msvc.exe';
    throw new Error(`Unsupported test platform: ${process.platform}/${process.arch}`);
}

function cachedBinaryPath(home, commandName) {
    const version = require('../package.json').version;
    return path.join(home, '.ccm', 'bin', `${commandName}-v${version}-${releaseTarget()}`);
}

function sha256(filePath) {
    return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

function writeVerifiedCache(home, sourcePath) {
    const binPath = cachedBinaryPath(home, 'ccm-cli');
    fs.mkdirSync(path.dirname(binPath), { recursive: true });
    fs.copyFileSync(sourcePath, binPath);
    fs.chmodSync(binPath, 0o755);
    fs.writeFileSync(`${binPath}.sha256`, `${sha256(binPath)}\n`);
    return binPath;
}

function tempConfig() {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ccm-installer-'));
    return {
        directory,
        configPath: path.join(directory, 'mcp.json')
    };
}

test('installer merges MCP config and preserves existing settings', () => {
    const { configPath } = tempConfig();
    fs.writeFileSync(
        configPath,
        JSON.stringify({ theme: 'dark', mcpServers: { existing: { command: 'safe' } } })
    );

    const changed = installJsonConfig(configPath, {
        command: 'npx',
        args: MCP_ARGS,
        env: MCP_ENV
    });

    assert.equal(changed, true);
    const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
    assert.equal(config.theme, 'dark');
    assert.equal(config.mcpServers.existing.command, 'safe');
    assert.equal(config.mcpServers['context-manager'].env.RUST_LOG, 'info');
    assert.ok(fs.existsSync(`${configPath}.bak`));
});

test('installer never overwrites malformed JSON', () => {
    const { configPath } = tempConfig();
    const malformed = '{"mcpServers":';
    fs.writeFileSync(configPath, malformed);

    assert.throws(
        () => installJsonConfig(configPath, { command: 'npx', args: [], env: {} }),
        /Could not parse/
    );
    assert.equal(fs.readFileSync(configPath, 'utf8'), malformed);
    assert.equal(fs.readFileSync(`${configPath}.bak`, 'utf8'), malformed);
});

test('atomic writer leaves valid JSON and no temporary file', () => {
    const { directory, configPath } = tempConfig();
    writeJsonAtomic(configPath, { ok: true });

    assert.deepEqual(JSON.parse(fs.readFileSync(configPath, 'utf8')), { ok: true });
    assert.deepEqual(fs.readdirSync(directory), ['mcp.json']);
});

test('generated MCP command pins package version and configures strict allowlist', () => {
    assert.match(MCP_ARGS[1], /^@senoldogann\/context-manager@\d+\.\d+\.\d+$/);
    assert.equal(MCP_ENV.RUST_LOG, 'info');
    assert.ok(MCP_ENV.CCM_PROJECT_ROOT, 'CCM_PROJECT_ROOT must be set');
    assert.equal(MCP_ENV.CCM_ALLOWED_ROOTS, MCP_ENV.CCM_PROJECT_ROOT);
    assert.equal(MCP_ENV.CCM_REQUIRE_ALLOWED_ROOTS, '1');
});

test('foreign npm package version cannot change the CCM release version', () => {
    const script = `const wrapper = require(${JSON.stringify(wrapperPath)}); process.stdout.write(wrapper.MCP_ARGS[1]);`;
    const result = spawnSync(process.execPath, ['-e', script], {
        encoding: 'utf8',
        env: { ...process.env, npm_package_version: '9.8.7', CCM_BINARY_VERSION: '' }
    });

    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, `@senoldogann/context-manager@${require('../package.json').version}`);
});

test('Codex installer replaces stale or disabled entry without invoking Codex binary', () => {
    const { configPath } = tempConfig();
    fs.writeFileSync(
        configPath,
        [
            'model = "gpt-5"',
            '',
            '[mcp_servers.context-manager]',
            'command = "old"',
            'enabled = false',
            '',
            '[mcp_servers.context-manager.env]',
            'CCM_PROJECT_ROOT = "/old"',
            '',
            '[mcp_servers.keep-me]',
            'command = "safe"',
            ''
        ].join('\n')
    );

    installCodexTomlConfig(configPath, '/projects/current', '0.2.1');

    const content = fs.readFileSync(configPath, 'utf8');
    assert.match(content, /model = "gpt-5"/);
    assert.match(content, /\[mcp_servers\.keep-me\]/);
    assert.match(content, /@senoldogann\/context-manager@0\.2\.1/);
    assert.match(content, /enabled = true/);
    assert.match(content, /CCM_PROJECT_ROOT = "\/projects\/current"/);
    assert.match(content, /CCM_ALLOWED_ROOTS = "\/projects\/current"/);
    assert.match(content, /CCM_REQUIRE_ALLOWED_ROOTS = "1"/);
    assert.doesNotMatch(content, /command = "old"/);
    assert.equal(
        content.match(/\[mcp_servers\.context-manager\]/g).length,
        1
    );
    assert.ok(fs.existsSync(`${configPath}.bak`));
});

test('Codex installer migrates quoted context-manager sections without duplicate TOML tables', () => {
    const { configPath } = tempConfig();
    fs.writeFileSync(
        configPath,
        [
            'model = "gpt-5"',
            '',
            '["mcp_servers"."context-manager"]',
            'command = "old"',
            '',
            '["mcp_servers"."context-manager".env]',
            'OLD = "1"',
            '',
            '[mcp_servers.keep-me]',
            'command = "safe"',
            ''
        ].join('\n')
    );

    installCodexTomlConfig(configPath, '/projects/current', '0.3.7');

    const content = fs.readFileSync(configPath, 'utf8');
    assert.doesNotMatch(content, /"mcp_servers"\."context-manager"/);
    assert.equal(content.match(/\[mcp_servers\.context-manager\]/g).length, 1);
    assert.equal(content.match(/\[mcp_servers\.context-manager\.env\]/g).length, 1);
    assert.match(content, /\[mcp_servers\.keep-me\]/);
    assert.doesNotMatch(content, /command = "old"/);
});

test('Codex installer preserves tables whose headers have trailing comments', () => {
    const { configPath } = tempConfig();
    const featureBlock = '[features] # user settings\nweb_search = true\n';
    fs.writeFileSync(
        configPath,
        `[mcp_servers.context-manager]\ncommand = "old"\n\n${featureBlock}`
    );

    installCodexTomlConfig(configPath, '/projects/current', '0.3.8');

    const content = fs.readFileSync(configPath, 'utf8');
    assert.match(content, /\[features\] # user settings\nweb_search = true/);
    assert.equal(content.match(/\[mcp_servers\.context-manager\]/g).length, 1);
});

test('JSON installer rejects array-shaped mcpServers without changing the original', () => {
    const { configPath } = tempConfig();
    const original = '{"mcpServers":[]}\n';
    fs.writeFileSync(configPath, original);

    assert.throws(
        () => installJsonConfig(configPath, { command: 'npx', args: [], env: {} }),
        /mcpServers must be a JSON object/
    );
    assert.equal(fs.readFileSync(configPath, 'utf8'), original);
});

test('JSON installer rejects a non-object document without changing the original', () => {
    for (const original of ['[]\n', 'null\n']) {
        const { configPath } = tempConfig();
        fs.writeFileSync(configPath, original);

        assert.throws(
            () => installJsonConfig(configPath, { command: 'npx', args: [], env: {} }),
            /top-level JSON value must be an object/
        );
        assert.equal(fs.readFileSync(configPath, 'utf8'), original);
    }
});

test('compressed release binary is restored byte-for-byte', async () => {
    const { directory } = tempConfig();
    const compressed = path.join(directory, 'ccm-mcp.gz');
    const restored = path.join(directory, 'ccm-mcp');
    const expected = Buffer.from('binary\0payload\nTürkçe ve Suomi 🧠');
    fs.writeFileSync(compressed, zlib.gzipSync(expected));

    await extractGzip(compressed, restored);

    assert.deepEqual(fs.readFileSync(restored), expected);
});

test('npm package ships the canonical agent skill', () => {
    const packageRoot = path.resolve(__dirname, '..');
    execFileSync(process.execPath, [path.join(packageRoot, 'scripts/sync-skill.js')]);

    const packagedSkill = fs.readFileSync(path.join(packageRoot, 'SKILL.md'), 'utf8');
    const canonicalSkill = fs.readFileSync(path.resolve(packageRoot, '..', 'SKILL.md'), 'utf8');
    assert.equal(packagedSkill, canonicalSkill);
});

test('installer writes the packaged agent skill atomically', () => {
    const { directory } = tempConfig();
    const sourcePath = path.join(directory, 'source-SKILL.md');
    const expected = '# CCM skill\n\nCurrent contract.\n';
    fs.writeFileSync(sourcePath, expected);

    installAgentSkill(directory, sourcePath);

    const installedPath = path.join(
        directory,
        '.agents',
        'skills',
        'context-manager',
        'SKILL.md'
    );
    assert.equal(fs.readFileSync(installedPath, 'utf8'), expected);
    assert.deepEqual(fs.readdirSync(path.dirname(installedPath)), ['SKILL.md']);
});

test('installer backs up a customized agent skill before replacing it', () => {
    const { directory } = tempConfig();
    const sourcePath = path.join(directory, 'source-SKILL.md');
    const skillDirectory = path.join(directory, '.agents', 'skills', 'context-manager');
    const skillPath = path.join(skillDirectory, 'SKILL.md');
    fs.mkdirSync(skillDirectory, { recursive: true });
    fs.writeFileSync(sourcePath, '# Canonical\n');
    fs.writeFileSync(skillPath, '# User customization\n');

    installAgentSkill(directory, sourcePath);

    assert.equal(fs.readFileSync(skillPath, 'utf8'), '# Canonical\n');
    assert.equal(fs.readFileSync(`${skillPath}.bak`, 'utf8'), '# User customization\n');
});

test('installer preserves every distinct agent skill customization', () => {
    const { directory } = tempConfig();
    const sourcePath = path.join(directory, 'source-SKILL.md');
    const skillDirectory = path.join(directory, '.agents', 'skills', 'context-manager');
    const skillPath = path.join(skillDirectory, 'SKILL.md');
    fs.mkdirSync(skillDirectory, { recursive: true });
    fs.writeFileSync(sourcePath, '# Canonical v1\n');
    fs.writeFileSync(skillPath, '# User customization v1\n');
    installAgentSkill(directory, sourcePath);

    fs.writeFileSync(sourcePath, '# Canonical v2\n');
    fs.writeFileSync(skillPath, '# User customization v2\n');
    installAgentSkill(directory, sourcePath);

    const backups = fs
        .readdirSync(skillDirectory)
        .filter((name) => name.startsWith('SKILL.md.bak'))
        .map((name) => fs.readFileSync(path.join(skillDirectory, name), 'utf8'));
    assert.deepEqual(new Set(backups), new Set([
        '# User customization v1\n',
        '# User customization v2\n'
    ]));
});

test('concurrent cache finalization replaces a corrupt target without races', async () => {
    const { directory } = tempConfig();
    const binPath = path.join(directory, 'ccm-cli');
    const firstTmp = path.join(directory, 'first.tmp');
    const secondTmp = path.join(directory, 'second.tmp');
    fs.writeFileSync(binPath, 'corrupt');
    fs.writeFileSync(firstTmp, 'verified-binary');
    fs.writeFileSync(secondTmp, 'verified-binary');

    await Promise.all([
        finalizeDownloadedBinary(binPath, firstTmp, true),
        finalizeDownloadedBinary(binPath, secondTmp, true)
    ]);

    assert.equal(fs.readFileSync(binPath, 'utf8'), 'verified-binary');
    assert.equal(await verifyCachedBinary(binPath), true);
    assert.equal(fs.existsSync(`${binPath}.lock`), false);
});

test('temporary download paths are unique even within one process', () => {
    const binPath = path.join(os.tmpdir(), 'ccm-binary');
    assert.notEqual(createUniqueTmpPath(binPath), createUniqueTmpPath(binPath));
});

test('verified binary cache works without a network request', () => {
    const { directory } = tempConfig();
    let expectedOutput = /^v\d+\./;
    let sourcePath = process.execPath;
    if (process.platform !== 'win32') {
        sourcePath = path.join(directory, 'cached-cli.sh');
        fs.writeFileSync(sourcePath, '#!/bin/sh\necho cached-cli-ok\n');
        fs.chmodSync(sourcePath, 0o755);
        expectedOutput = /^cached-cli-ok/;
    }
    writeVerifiedCache(directory, sourcePath);

    const result = spawnSync(process.execPath, [wrapperPath, '--version'], {
        encoding: 'utf8',
        env: {
            ...process.env,
            HOME: directory,
            USERPROFILE: directory,
            npm_package_version: '9.8.7',
            CCM_BINARY_VERSION: '',
            CCM_DOWNLOAD_TIMEOUT_MS: '1'
        }
    });

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, expectedOutput);
    assert.doesNotMatch(result.stdout + result.stderr, /Downloading/);
});

test('corrupted binary cache fails sidecar verification', async () => {
    const { directory } = tempConfig();
    const binPath = writeVerifiedCache(directory, process.execPath);
    fs.appendFileSync(binPath, 'corruption');

    assert.equal(await verifyCachedBinary(binPath), false);
});

test('wrapper returns nonzero when the cached binary terminates by signal', () => {
    if (process.platform === 'win32') return;
    const { directory } = tempConfig();
    const scriptPath = path.join(directory, 'signal.sh');
    fs.writeFileSync(scriptPath, '#!/bin/sh\nkill -TERM $$\n');
    fs.chmodSync(scriptPath, 0o755);
    writeVerifiedCache(directory, scriptPath);

    const result = spawnSync(process.execPath, [wrapperPath, '--version'], {
        encoding: 'utf8',
        env: {
            ...process.env,
            HOME: directory,
            USERPROFILE: directory,
            CCM_BINARY_VERSION: ''
        }
    });

    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /terminated by signal SIGTERM/);
});

test('release asset job only runs for tag refs', () => {
    const workflowPath = path.resolve(packageRoot, '..', '.github', 'workflows', 'release.yml');
    const workflow = fs.readFileSync(workflowPath, 'utf8');
    const releaseJob = workflow.split('\n  release:')[1].split('\n  prepare-npm:')[0];

    assert.match(releaseJob, /\n    if: github\.ref_type == 'tag'\n/);
});

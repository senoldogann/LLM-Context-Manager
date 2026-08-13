const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const zlib = require('node:zlib');
const { execFileSync } = require('node:child_process');

const {
    MCP_ARGS,
    MCP_ENV,
    extractGzip,
    installAgentSkill,
    installCodexTomlConfig,
    installJsonConfig,
    writeJsonAtomic
} = require('../bin/ccm.js');

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

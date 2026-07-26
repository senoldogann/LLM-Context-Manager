const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');

const {
    MCP_ARGS,
    MCP_ENV,
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
    assert.equal(config.mcpServers['context-manager'].env.CCM_REQUIRE_ALLOWED_ROOTS, '1');
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

test('generated MCP command pins package version and narrows project access', () => {
    assert.match(MCP_ARGS[1], /^@senoldogann\/context-manager@\d+\.\d+\.\d+$/);
    assert.equal(MCP_ENV.CCM_REQUIRE_ALLOWED_ROOTS, '1');
    assert.equal(MCP_ENV.CCM_ALLOWED_ROOTS, process.cwd());
    assert.equal(MCP_ENV.CCM_PROJECT_ROOT, process.cwd());
});

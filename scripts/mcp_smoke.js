#!/usr/bin/env node

const { spawn } = require('node:child_process');

const binary = process.argv[2];
if (!binary) {
    throw new Error('Usage: node scripts/mcp_smoke.js <ccm-mcp-binary>');
}

// Varsayılan keşif yüzeyi kasıtlı olarak küçüktür (mcp/src/server.rs): eski yapısal
// araçlar yalnızca CCM_MCP_LEGACY_TOOLS=1 ile listelenir. İki yüzey de doğrulanır.
const surfaces = [
    {
        label: 'default',
        legacyTools: '0',
        expected: ['context', 'index_project']
    },
    {
        label: 'legacy',
        legacyTools: '1',
        expected: [
            'context',
            'map',
            'explain',
            'find_usages',
            'impact_of_change',
            'search_code',
            'find_nodes',
            'trace_call_chain',
            'diff_context',
            'index_project',
            'index_now'
        ]
    }
];

const initializeRequest = {
    jsonrpc: '2.0',
    id: 1,
    method: 'initialize',
    params: { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'smoke', version: '1' } }
};
const toolsListRequest = { jsonrpc: '2.0', id: 2, method: 'tools/list' };

function verifySurface(surface, initialize, toolsList) {
    if (initialize?.result?.serverInfo?.name !== 'ccm-mcp') {
        throw new Error('MCP initialize response is invalid');
    }
    const names = (toolsList?.result?.tools ?? []).map((tool) => tool.name);
    for (const name of surface.expected) {
        if (!names.includes(name)) {
            throw new Error(`MCP tool is missing (${surface.label} surface): ${name}`);
        }
    }
}

function smokeSurface(surface) {
    return new Promise((resolve, reject) => {
        const child = spawn(binary, [], {
            env: {
                ...process.env,
                RUST_LOG: 'error',
                CCM_PROJECT_ROOT: process.cwd(),
                CCM_ALLOWED_ROOTS: process.cwd(),
                CCM_REQUIRE_ALLOWED_ROOTS: '1',
                CCM_MCP_LEGACY_TOOLS: surface.legacyTools
            },
            stdio: ['pipe', 'pipe', 'inherit']
        });

        let buffer = '';
        let settled = false;
        const responses = new Map();
        const finish = (error) => {
            if (settled) {
                return;
            }
            settled = true;
            clearTimeout(timeout);
            child.kill();
            if (error) {
                reject(error);
            } else {
                resolve();
            }
        };
        const timeout = setTimeout(() => {
            finish(new Error(`MCP smoke test timed out (${surface.label} surface)`));
        }, 15000);

        child.stdout.setEncoding('utf8');
        child.stdout.on('data', (chunk) => {
            try {
                buffer += chunk;
                const lines = buffer.split('\n');
                buffer = lines.pop();
                for (const line of lines.filter(Boolean)) {
                    const response = JSON.parse(line);
                    responses.set(response.id, response);
                }
                if (responses.has(1) && responses.has(2)) {
                    verifySurface(surface, responses.get(1), responses.get(2));
                    finish(null);
                }
            } catch (error) {
                finish(error);
            }
        });

        child.on('error', finish);
        child.stdin.on('error', finish);
        child.stdin.write(`${JSON.stringify(initializeRequest)}\n`);
        child.stdin.write(`${JSON.stringify(toolsListRequest)}\n`);
    });
}

async function main() {
    for (const surface of surfaces) {
        await smokeSurface(surface);
        console.log(`MCP smoke passed (${surface.label}): initialize + ${surface.expected.length} tools`);
    }
}

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});

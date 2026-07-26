#!/usr/bin/env node

const { spawn } = require('node:child_process');

const binary = process.argv[2];
if (!binary) {
    throw new Error('Usage: node scripts/mcp_smoke.js <ccm-mcp-binary>');
}

const child = spawn(binary, [], {
    env: {
        ...process.env,
        RUST_LOG: 'error',
        CCM_PROJECT_ROOT: process.cwd(),
        CCM_ALLOWED_ROOTS: process.cwd(),
        CCM_REQUIRE_ALLOWED_ROOTS: '1'
    },
    stdio: ['pipe', 'pipe', 'inherit']
});

let buffer = '';
const responses = new Map();
const timeout = setTimeout(() => {
    child.kill();
    throw new Error('MCP smoke test timed out');
}, 15000);

child.stdout.setEncoding('utf8');
child.stdout.on('data', (chunk) => {
    buffer += chunk;
    const lines = buffer.split('\n');
    buffer = lines.pop();
    for (const line of lines.filter(Boolean)) {
        const response = JSON.parse(line);
        responses.set(response.id, response);
        if (responses.has(1) && responses.has(2)) {
            const initialize = responses.get(1);
            const tools = responses.get(2)?.result?.tools ?? [];
            const names = tools.map((tool) => tool.name);
            const expected = [
                'get_context',
                'search_code',
                'find_nodes',
                'read_graph',
                'index_project',
                'find_usages',
                'trace_call_chain',
                'impact_of_change',
                'diff_context'
            ];
            if (initialize?.result?.serverInfo?.name !== 'ccm-mcp') {
                throw new Error('MCP initialize response is invalid');
            }
            for (const name of expected) {
                if (!names.includes(name)) {
                    throw new Error(`MCP tool is missing: ${name}`);
                }
            }
            clearTimeout(timeout);
            child.kill();
            console.log(`MCP smoke passed: initialize + ${expected.length} tools`);
        }
    }
});

child.on('error', (error) => {
    clearTimeout(timeout);
    throw error;
});

child.stdin.write(`${JSON.stringify({
    jsonrpc: '2.0',
    id: 1,
    method: 'initialize',
    params: { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'smoke', version: '1' } }
})}\n`);
child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id: 2, method: 'tools/list' })}\n`);

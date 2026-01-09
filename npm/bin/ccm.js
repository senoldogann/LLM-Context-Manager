#!/usr/bin/env node

const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');
const https = require('https');

const VERSION = '0.1.7';
const REPO = 'senoldogann/LLM-Context-Manager';
const BIN_DIR = path.join(os.homedir(), '.ccm', 'bin');

async function installMcp() {
    const configPaths = [];
    const home = os.homedir();

    if (os.platform() === 'darwin') {
        // MacOS Paths
        configPaths.push(path.join(home, 'Library', 'Application Support', 'Claude', 'claude_desktop_config.json'));
        configPaths.push(path.join(home, '.gemini', 'antigravity', 'mcp_config.json'));
        // VS Code Extensions (Cline & Roo Code)
        configPaths.push(path.join(home, 'Library', 'Application Support', 'Code', 'User', 'globalStorage', 'saoudrizwan.claude-dev', 'settings', 'cline_mcp_settings.json'));
        configPaths.push(path.join(home, 'Library', 'Application Support', 'Code', 'User', 'globalStorage', 'rooveterinaryinc.roo-cline', 'settings', 'cline_mcp_settings.json'));
    } else if (os.platform() === 'win32') {
        // Windows Paths
        const appData = process.env.APPDATA || '';
        configPaths.push(path.join(appData, 'Claude', 'claude_desktop_config.json'));
        // VS Code Extensions on Windows
        configPaths.push(path.join(process.env.USERPROFILE || '', 'AppData', 'Roaming', 'Code', 'User', 'globalStorage', 'saoudrizwan.claude-dev', 'settings', 'cline_mcp_settings.json'));
    } else if (os.platform() === 'linux') {
        // Linux Paths
        configPaths.push(path.join(home, '.config', 'Claude', 'claude_desktop_config.json'));
        configPaths.push(path.join(home, '.config', 'Code', 'User', 'globalStorage', 'saoudrizwan.claude-dev', 'settings', 'cline_mcp_settings.json'));
    }

    const mcpConfig = {
        "command": "npx",
        "args": ["-y", "@senoldogann/context-manager", "mcp"],
        "env": {
            "RUST_LOG": "info"
        }
    };

    let installedCount = 0;
    for (const configPath of configPaths) {
        const dir = path.dirname(configPath);
        if (fs.existsSync(dir)) {
            let config = { mcpServers: {} };
            if (fs.existsSync(configPath)) {
                try {
                    const content = fs.readFileSync(configPath, 'utf8');
                    config = JSON.parse(content);
                    // Backup
                    fs.copyFileSync(configPath, `${configPath}.bak`);
                } catch (e) {
                    console.warn(`[CCM] Could not parse ${configPath}, creating backup and starting fresh section.`);
                }
            }

            if (!config.mcpServers) config.mcpServers = {};
            config.mcpServers["context-manager"] = mcpConfig;

            fs.writeFileSync(configPath, JSON.stringify(config, null, 2));
            console.log(`[CCM] ✓ Successfully updated: ${configPath}`);
            installedCount++;
        }
    }

    if (installedCount === 0) {
        console.log("[CCM] No supported MCP config directories found.");
        console.log("[CCM] Please add this to your mcp_config.json manually:");
        console.log(JSON.stringify({ "context-manager": mcpConfig }, null, 2));
    } else {
        console.log("[CCM] Installation complete! Restart your AI editor to see the changes.");
    }
}

async function getBinary() {
    const platform = os.platform(); // darwin, linux, win32
    const arch = os.arch(); // x64, arm64

    let target = '';
    if (platform === 'darwin') {
        target = arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
    } else if (platform === 'linux') {
        target = 'x86_64-unknown-linux-gnu';
    } else if (platform === 'win32') {
        target = 'x86_64-pc-windows-msvc.exe';
    }

    // Detect which tool to run
    let commandName = 'ccm-cli'; // Default
    const binName = path.basename(process.argv[1]);

    if (binName.includes('mcp')) {
        commandName = 'ccm-mcp';
    } else if (process.argv[2] === 'mcp') {
        // Handle: npx @ccm/context-manager mcp
        commandName = 'ccm-mcp';
        process.argv.splice(2, 1); // Remove 'mcp' from args to be passed to binary
    } else if (process.argv[2] === 'install') {
        await installMcp();
        process.exit(0);
    }

    const binFilename = `${commandName}-${target}`;
    const binPath = path.join(BIN_DIR, binFilename);

    if (fs.existsSync(binPath)) {
        return binPath;
    }

    console.log(`[CCM] Binary not found. Downloading ${commandName} for ${target}...`);

    if (!fs.existsSync(BIN_DIR)) {
        fs.mkdirSync(BIN_DIR, { recursive: true });
    }

    const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${commandName}-${target}`;

    await downloadFile(url, binPath);
    fs.chmodSync(binPath, '755');

    return binPath;
}

function downloadFile(url, dest) {
    return new Promise((resolve, reject) => {
        const file = fs.createWriteStream(dest);
        https.get(url, (response) => {
            if (response.statusCode === 302 || response.statusCode === 301) {
                downloadFile(response.headers.location, dest).then(resolve).catch(reject);
                return;
            }
            if (response.statusCode !== 200) {
                reject(new Error(`Failed to download: ${response.statusCode}`));
                return;
            }
            response.pipe(file);
            file.on('finish', () => {
                file.close(resolve);
            });
        }).on('error', (err) => {
            fs.unlink(dest, () => { });
            reject(err);
        });
    });
}

async function main() {
    try {
        const binPath = await getBinary();
        const args = process.argv.slice(2);

        const child = spawn(binPath, args, {
            stdio: 'inherit',
            env: {
                ...process.env,
                // Ensure MCP knows its project root if it can
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

#!/usr/bin/env node

const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');
const os = require('os');
const https = require('https');

const VERSION = "0.1.19";
const REPO = 'senoldogann/LLM-Context-Manager';
const BIN_DIR = path.join(os.homedir(), '.ccm', 'bin');

async function installMcp() {
    const configPaths = [];
    const home = os.homedir();

    if (os.platform() === 'darwin') {
        configPaths.push(path.join(home, 'Library', 'Application Support', 'Claude', 'claude_desktop_config.json'));
        configPaths.push(path.join(home, '.gemini', 'antigravity', 'mcp_config.json'));
        configPaths.push(path.join(home, 'Library', 'Application Support', 'Code', 'User', 'globalStorage', 'saoudrizwan.claude-dev', 'settings', 'cline_mcp_settings.json'));
        configPaths.push(path.join(home, 'Library', 'Application Support', 'Code', 'User', 'globalStorage', 'rooveterinaryinc.roo-cline', 'settings', 'cline_mcp_settings.json'));
    } else if (os.platform() === 'win32') {
        const appData = process.env.APPDATA || '';
        configPaths.push(path.join(appData, 'Claude', 'claude_desktop_config.json'));
        configPaths.push(path.join(process.env.USERPROFILE || '', 'AppData', 'Roaming', 'Code', 'User', 'globalStorage', 'saoudrizwan.claude-dev', 'settings', 'cline_mcp_settings.json'));
    } else if (os.platform() === 'linux') {
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

    console.log("[CCM] Pre-downloading binaries for all tools...");
    await getBinaryFor('ccm-cli');
    await getBinaryFor('ccm-mcp');

    let installedCount = 0;
    for (const configPath of configPaths) {
        const dir = path.dirname(configPath);
        if (fs.existsSync(dir)) {
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
    const tmpPath = `${binPath}.tmp`;

    try {
        await downloadFile(url, tmpPath);
        fs.chmodSync(tmpPath, '755');
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
        https.get(url, (response) => {
            if (response.statusCode === 302 || response.statusCode === 301) {
                return downloadFile(response.headers.location, dest).then(resolve).catch(reject);
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

// Ubah scriptlet dan resource pengganti uBlock Origin menjadi resources.json untuk adblock-rust
// (format yang sama dengan yang dipakai Brave). Dijalankan oleh CI:
//   node tools/build-resources.mjs <folder repo uBlock> <berkas keluaran>
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const [ubo, out] = process.argv.slice(2);
if (!ubo || !out) {
    console.error('pakai: node tools/build-resources.mjs <uBlock> <resources.json>');
    process.exit(2);
}
const b64 = (s) => Buffer.from(s).toString('base64');
const resources = [];

// 1. Scriptlet (+js): fungsi bernama; adblock-rust memanggilnya dengan argumen filter.
const { builtinScriptlets } = await import(pathToFileURL(join(ubo, 'src/js/resources/scriptlets.js')).href);
let skipped = 0;
for (const s of builtinScriptlets) {
    const code = s.fn.toString();
    if (!/^function\s+[^\s(){}]+\s*\(/.test(code)) { skipped++; continue; }
    const isFn = s.name.endsWith('.fn');
    const aliases = (s.aliases || []).filter((a) => a !== s.name);
    const r = {
        name: s.name,
        aliases,
        kind: { mime: isFn ? 'fn/javascript' : 'application/javascript' },
        content: b64(code),
    };
    if (s.dependencies && s.dependencies.length) r.dependencies = s.dependencies;
    // Scriptlet "trusted-*" hanya boleh dipakai daftar tepercaya (uBO/Brave): bit izin 1.
    if (s.requiresTrust) r.permission = 1;
    resources.push(r);
}

// 2. Resource pengganti ($redirect): berkas di web_accessible_resources.
const mime = (f) => ({ js: 'application/javascript', css: 'text/css', gif: 'image/gif', png: 'image/png',
    html: 'text/html', json: 'application/json', mp3: 'audio/mp3', mp4: 'video/mp4', txt: 'text/plain', xml: 'text/xml' })[f.split('.').pop()] || 'application/octet-stream';
const { default: redirects } = await import(pathToFileURL(join(ubo, 'src/js/redirect-resources.js')).href);
for (const [name, props] of redirects) {
    let data;
    try { data = readFileSync(join(ubo, 'src/web_accessible_resources', name)); } catch { continue; }
    const alias = props.alias === undefined ? [] : Array.isArray(props.alias) ? props.alias : [props.alias];
    resources.push({ name, aliases: alias, kind: { mime: mime(name) }, content: data.toString('base64') });
}

writeFileSync(out, JSON.stringify(resources));
console.log(`${resources.length} resource (${builtinScriptlets.length - skipped} scriptlet, ${skipped} dilewati) -> ${out}`);

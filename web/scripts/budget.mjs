import { readdir, readFile, writeFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { gzipSync } from 'node:zlib'
import { resolve, relative } from 'node:path'
const root = resolve('dist')
const paths = []
async function visit(dir) { for (const entry of await readdir(dir, { withFileTypes: true })) { const path = resolve(dir, entry.name); if (entry.isDirectory()) await visit(path); else if (entry.isFile()) paths.push(path); else throw new Error('Unsupported static entry') } }
await visit(root)
const assets = []; let total = 0, compressedJsCss = 0
for (const path of paths.sort()) {
  const name = relative(root, path).replaceAll('\\', '/')
  if (name === 'fc-assets.json' || name === 'build-budget.json') continue
  if (!(name === 'index.html' || /^assets\/[A-Za-z0-9_-][A-Za-z0-9._-]*\.(?:js|css|svg|png)$/.test(name)) || name.includes('..')) throw new Error(`Unsupported deployment path: ${name}`)
  const bytes = await readFile(path); total += bytes.byteLength
  if (/\.(js|css)$/.test(name)) compressedJsCss += gzipSync(bytes, { level: 9 }).byteLength
  assets.push({ path: name, sizeBytes: bytes.byteLength, sha256: createHash('sha256').update(bytes).digest('hex') })
}
if (!assets.some(a => a.path === 'index.html') || assets.length > 64 || total > 5 * 1024 * 1024 || compressedJsCss > 512 * 1024) throw new Error('Static build budget exceeded')
await writeFile(resolve(root, 'fc-assets.json'), JSON.stringify({ schemaVersion: 1, apiVersion: 1, uiCompatibilityVersion: 1, assets }, null, 2) + '\n')
const report = { schemaVersion: 1, status: 'PASS', assetCount: assets.length, totalBytes: total, totalLimitBytes: 5 * 1024 * 1024, gzipJsCssBytes: compressedJsCss, gzipJsCssLimitBytes: 512 * 1024, scope: 'static build only; listener, wire TLS, real browser pairing not established by this report' }
// Build evidence stays outside dist: only declared application files are shipped.
await writeFile('build-budget.json', JSON.stringify(report, null, 2) + '\n')
process.stdout.write(JSON.stringify(report, null, 2) + '\n')

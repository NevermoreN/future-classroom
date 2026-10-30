import { readFile, writeFile, readdir } from 'node:fs/promises'
import { resolve } from 'node:path'
const lock = JSON.parse(await readFile('package-lock.json', 'utf8'))
let output = 'FutureClassroom teacher static bundle — third-party notices\n\nSource: exact package-lock.json production dependency closure; shadcn/ui copied components.\n\n'
for (const [path, metadata] of Object.entries(lock.packages).sort(([a], [b]) => a.localeCompare(b, 'en'))) {
  if (!path || metadata.dev === true) continue
  const dir = resolve(path), names = await readdir(dir)
  const files = names.filter(name => /^(licen[cs]e|copying|notice)(\.|$)/i.test(name)).sort()
  if (!files.length) throw new Error(`Missing license text: ${path}`)
  output += `\n===== ${path.replace(/^node_modules\//, '')} ${metadata.version} (${metadata.license}) =====\n`
  for (const file of files) output += `\n${file}\n${await readFile(resolve(dir, file), 'utf8')}\n`
}
output += '\n===== shadcn/ui source components =====\n' + await readFile('SHADCN-LICENSE.md', 'utf8')
await writeFile('THIRD-PARTY-NOTICES.txt', output)

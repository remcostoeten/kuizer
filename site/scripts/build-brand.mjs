import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { copyFile, mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises'
import { dirname, extname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import sharp from 'sharp'

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const brand = join(site, 'public/brand')
const root = join(brand, 'sweep')
const source = join(root, 'source')
const sizes = [16, 24, 32, 48, 64, 96, 128, 180, 192, 256, 512, 1024]
const colors = { black: '#171717', white: '#fafafa', orange: '#f97844' }

function svg(width, height, body) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">${body}</svg>`
}

async function save(relative, data) {
  const path = join(root, relative)
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, data)
}

async function png(relative, input, width, height = width) {
  await save(relative, await sharp(input).resize(width, height).png({ compressionLevel: 9 }).toBuffer())
}

function distance(point, first, last) {
  const dx = last[0] - first[0]
  const dy = last[1] - first[1]
  if (dx === 0 && dy === 0) return Math.hypot(point[0] - first[0], point[1] - first[1])
  return (
    Math.abs(dy * point[0] - dx * point[1] + last[0] * first[1] - last[1] * first[0]) / Math.hypot(dx, dy)
  )
}

function simplify(points) {
  let furthest = 0
  let maximum = 0.65
  for (let index = 1; index < points.length - 1; index++) {
    const value = distance(points[index], points[0], points.at(-1))
    if (value > maximum) {
      maximum = value
      furthest = index
    }
  }
  if (furthest === 0) return [points[0], points.at(-1)]
  return [...simplify(points.slice(0, furthest + 1)).slice(0, -1), ...simplify(points.slice(furthest))]
}

async function traceMark() {
  const width = 512
  const alpha = await sharp(join(source, 'mono.png'))
    .resize(width, width)
    .extractChannel('alpha')
    .raw()
    .toBuffer()
  const stride = width + 1
  const edges = new Map()

  function filled(x, y) {
    return x >= 0 && y >= 0 && x < width && y < width && alpha[y * width + x] >= 128
  }

  function edge(x, y, endX, endY) {
    const key = y * stride + x
    const ends = edges.get(key) ?? []
    ends.push(endY * stride + endX)
    edges.set(key, ends)
  }

  for (let y = 0; y < width; y++) {
    for (let x = 0; x < width; x++) {
      if (!filled(x, y)) continue
      if (!filled(x, y - 1)) edge(x, y, x + 1, y)
      if (!filled(x + 1, y)) edge(x + 1, y, x + 1, y + 1)
      if (!filled(x, y + 1)) edge(x + 1, y + 1, x, y + 1)
      if (!filled(x - 1, y)) edge(x, y + 1, x, y)
    }
  }

  const paths = []
  while (edges.size > 0) {
    const start = edges.keys().next().value
    const points = []
    let current = start
    do {
      points.push([current % stride, Math.floor(current / stride)])
      const ends = edges.get(current)
      if (!ends?.length) throw new Error('The monochrome outline contains an open contour.')
      const next = ends.pop()
      if (ends.length === 0) edges.delete(current)
      current = next
    } while (current !== start)
    if (points.length < 20) continue
    const midpoint = Math.floor(points.length / 2)
    const outline = [
      ...simplify(points.slice(0, midpoint + 1)).slice(0, -1),
      ...simplify([...points.slice(midpoint), points[0]]),
    ]
    paths.push(`M${outline.map((point) => point.join(',')).join('L')}Z`)
  }
  if (paths.length !== 4)
    throw new Error(`Expected a ribbon and three blocks, found ${paths.length} contours.`)
  return `<path fill-rule="evenodd" d="${paths.join('')}"/>`
}

function wordmarkAt(wordmark, x, y, width, height, color) {
  return wordmark
    .replace('<svg ', `<svg x="${x}" y="${y}" width="${width}" height="${height}" `)
    .replace('currentColor', color)
}

function imageAt(buffer, x, y, width, height = width) {
  return `<image x="${x}" y="${y}" width="${width}" height="${height}" href="data:image/png;base64,${buffer.toString('base64')}"/>`
}

function ico(images) {
  const header = Buffer.alloc(6 + images.length * 16)
  header.writeUInt16LE(1, 2)
  header.writeUInt16LE(images.length, 4)
  let offset = header.length
  images.forEach(({ size, data }, index) => {
    const entry = 6 + index * 16
    header[entry] = size === 256 ? 0 : size
    header[entry + 1] = size === 256 ? 0 : size
    header.writeUInt16LE(1, entry + 4)
    header.writeUInt16LE(32, entry + 6)
    header.writeUInt32LE(data.length, entry + 8)
    header.writeUInt32LE(offset, entry + 12)
    offset += data.length
  })
  return Buffer.concat([header, ...images.map((entry) => entry.data)])
}

async function icns() {
  const chunks = []
  for (const [size, type] of [
    [16, 'icp4'],
    [32, 'icp5'],
    [64, 'icp6'],
    [128, 'ic07'],
    [256, 'ic08'],
    [512, 'ic09'],
    [1024, 'ic10'],
  ]) {
    const data = await readFile(join(root, `icons/app-${size}.png`))
    const header = Buffer.alloc(8)
    header.write(type, 0, 'ascii')
    header.writeUInt32BE(data.length + 8, 4)
    chunks.push(header, data)
  }
  const header = Buffer.alloc(8)
  header.write('icns', 0, 'ascii')
  header.writeUInt32BE(8 + chunks.reduce((length, chunk) => length + chunk.length, 0), 4)
  return Buffer.concat([header, ...chunks])
}

async function main() {
  sharp.concurrency(2)
  const master = await readFile(join(source, 'app-icon.png'))
  const mark = await sharp(join(source, 'mark.png')).resize(1024).png().toBuffer()
  const wordmark = (await readFile(join(source, 'wordmark.svg'), 'utf8')).trim()
  const outline = await traceMark()
  await save('marks/full-color.png', mark)
  await save('marks/full-color.webp', await sharp(mark).webp({ quality: 95, alphaQuality: 100 }).toBuffer())

  for (const [name, color] of Object.entries(colors)) {
    const vector = svg(512, 512, `<g fill="${color}">${outline}</g>`)
    await save(`marks/mono-${name}.svg`, vector)
    await png(`marks/mono-${name}.png`, Buffer.from(vector), 1024)
    await save(`wordmarks/${name}.svg`, wordmark.replace('currentColor', color))
  }

  for (const size of sizes) await png(`icons/app-${size}.png`, master, size)
  for (const size of [192, 512]) {
    const icon = svg(
      size,
      size,
      `<rect width="${size}" height="${size}" fill="#fff0da"/>${imageAt(mark, size * 0.18, size * 0.18, size * 0.64)}`,
    )
    await png(`icons/maskable-${size}.png`, Buffer.from(icon), size)
  }
  await save('icons/kuizer.icns', await icns())

  for (const [theme, color] of [
    ['light', colors.black],
    ['dark', colors.white],
  ]) {
    for (const layout of ['horizontal', 'stacked']) {
      const stacked = layout === 'stacked'
      const width = stacked ? 1000 : 1600
      const height = stacked ? 1000 : 480
      const artwork = stacked ? imageAt(mark, 200, 24, 600) : imageAt(mark, 24, 24, 432)
      const letters = stacked
        ? wordmarkAt(wordmark, 140, 730, 720, 170, color)
        : wordmarkAt(wordmark, 500, 136, 1040, 208, color)
      const lockup = svg(width, height, artwork + letters)
      await save(`logos/${layout}-${theme}.svg`, lockup)
      await png(`logos/${layout}-${theme}.png`, Buffer.from(lockup), width, height)
      await save(
        `logos/${layout}-${theme}.webp`,
        await sharp(Buffer.from(lockup)).webp({ quality: 95, alphaQuality: 100 }).toBuffer(),
      )
      const monoMark = `<g transform="translate(${stacked ? 200 : 24},24) scale(${(stacked ? 600 : 432) / 512})" fill="${color}">${outline}</g>`
      const monochrome = svg(width, height, monoMark + letters)
      await save(`logos/${layout}-mono-${theme}.svg`, monochrome)
      await png(`logos/${layout}-mono-${theme}.png`, Buffer.from(monochrome), width, height)
    }
  }

  const faviconImages = []
  for (const size of [16, 32, 48, 64, 128, 256]) {
    const input =
      size <= 48 ? Buffer.from(svg(512, 512, `<g fill="${colors.orange}">${outline}</g>`)) : master
    const data = await sharp(input).resize(size, size).png().toBuffer()
    await save(`favicons/favicon-${size}.png`, data)
    faviconImages.push({ size, data })
  }
  await save('favicons/favicon.ico', ico(faviconImages))
  await png('favicons/apple-touch-icon.png', master, 180)
  for (const [theme, color] of [
    ['light', colors.black],
    ['dark', colors.white],
  ]) {
    await save(`favicons/favicon-${theme}.svg`, svg(512, 512, `<g fill="${color}">${outline}</g>`))
  }

  for (const [name, width, height] of [
    ['opengraph', 1200, 630],
    ['square', 1200, 1200],
    ['github', 1280, 640],
    ['banner', 1500, 500],
    ['readme', 1280, 320],
  ]) {
    for (const theme of ['light', 'dark']) {
      const background = theme === 'light' ? '#fff8ef' : '#111111'
      const foreground = theme === 'light' ? '#171717' : '#fafafa'
      const muted = theme === 'light' ? '#65564d' : '#b9aaa0'
      const square = name === 'square'
      const compact = height <= 500
      const iconSize = square ? 620 : compact ? height * 0.72 : 370
      const iconX = square ? (width - iconSize) / 2 : width - iconSize - 52
      const iconY = square ? 64 : (height - iconSize) / 2
      const wordX = square ? 280 : 64
      const wordY = square ? 768 : compact ? 72 : 80
      const wordWidth = square ? 640 : compact ? 380 : 400
      const headingY = square ? 1020 : compact ? 208 : 300
      const headlineSize = square ? 48 : compact ? 48 : 64
      const body = `<rect width="${width}" height="${height}" fill="${background}"/>${imageAt(master, iconX, iconY, iconSize)}${wordmarkAt(wordmark, wordX, wordY, wordWidth, wordWidth * 0.2, foreground)}<text x="${square ? width / 2 : 64}" y="${headingY}" fill="${foreground}" font-family="Inter, sans-serif" font-weight="600" font-size="${headlineSize}" letter-spacing="-2"${square ? ' text-anchor="middle"' : ''}>Reclaim your disk.</text>${compact || square ? '' : `<text x="64" y="365" fill="${muted}" font-family="IBM Plex Mono, monospace" font-size="21">node_modules, target, .next and caches.</text>`}<text x="${square ? width / 2 : 64}" y="${height - (square ? 62 : 54)}" fill="${muted}" font-family="IBM Plex Mono, monospace" font-size="${name === 'readme' ? 18 : 22}"${square ? ' text-anchor="middle"' : ''}>kuizer.app</text>`
      const input = Buffer.from(svg(width, height, body))
      await png(`social/${name}-${theme}.png`, input, width, height)
      await save(
        `social/${name}-${theme}.jpg`,
        await sharp(input).jpeg({ quality: 92, mozjpeg: true }).toBuffer(),
      )
    }
  }
  for (const theme of ['light', 'dark']) {
    await png(`email/signature-${theme}.png`, join(root, `logos/horizontal-${theme}.png`), 600, 180)
  }

  await copyFile(join(root, 'icons/app-512.png'), join(brand, 'kuizer.png'))
  await sharp(master).resize(128).webp({ quality: 92 }).toFile(join(brand, 'kuizer.webp'))
  await copyFile(join(root, 'favicons/favicon-32.png'), join(site, 'src/app/icon.png'))
  await copyFile(join(root, 'favicons/favicon.ico'), join(site, 'src/app/favicon.ico'))
  await copyFile(join(root, 'favicons/apple-touch-icon.png'), join(site, 'src/app/apple-icon.png'))

  const packaging = resolve(site, '../packaging')
  for (const size of [32, 128, 256, 512]) {
    await copyFile(join(root, `icons/app-${size}.png`), join(packaging, `icons/${size}x${size}.png`))
  }
  await copyFile(join(root, 'marks/mono-orange.svg'), join(packaging, 'kuizer-gui.svg'))

  const files = []
  async function collect(directory, prefix = '') {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const relative = join(prefix, entry.name)
      if (entry.isDirectory()) await collect(join(directory, entry.name), relative)
      else if (entry.name !== 'manifest.json') {
        const data = await readFile(join(directory, entry.name))
        const format = extname(entry.name).slice(1)
        const dimensions = ['png', 'webp', 'jpg'].includes(format) ? await sharp(data).metadata() : {}
        files.push({
          path: relative,
          format,
          bytes: data.length,
          width: dimensions.width,
          height: dimensions.height,
          sha256: createHash('sha256').update(data).digest('hex'),
          ...(format === 'svg'
            ? {
                artwork: data.includes('<image')
                  ? 'raster mark with outlined vector typography'
                  : 'vector paths',
              }
            : {}),
        })
      }
    }
  }
  await collect(root)
  await save(
    'manifest.json',
    JSON.stringify(
      {
        identity: 'kuizer / Sweep',
        selectedConcept: 'Round 01 / Sweep',
        primaryColor: colors.orange,
        files: files.sort((a, b) => a.path.localeCompare(b.path)),
      },
      null,
      2,
    ) + '\n',
  )
  const archive = join(brand, 'kuizer-sweep-brand-kit.zip')
  execFileSync('zip', ['-q', '-r', '-FS', archive, 'sweep'], { cwd: brand })
  console.log(
    `Exported ${files.length} files. Kit: ${archive} (${Math.round((await stat(archive)).size / 1024 / 1024)} MB)`,
  )
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})

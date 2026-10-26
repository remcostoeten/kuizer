'use client'

import { useEffect, useRef } from 'react'
import { readTheme } from '@/shared/helpers/theme'

const brand = [210, 86, 17] as const
const columnChars = 32
const gapChars = 6
const scanLine = 0.34
const rowsPerSecond = 1.1
const foundFor = 900
const leaveFor = 420

const projectNames = [
  'kuizer',
  'dashboard',
  'api-gateway',
  'blog',
  'infra',
  'mobile-app',
  'design-system',
  'scraper',
  'notes',
  'ml-sandbox',
  'cli-tools',
  'website',
  'editor',
  'game',
  'docs',
]

type Spec = {
  name: string
  reclaim?: boolean
  children?: Spec[]
}

const stacks: Spec[][] = [
  [
    { name: 'src/', children: [{ name: 'index.ts' }, { name: 'utils/' }] },
    { name: 'package.json' },
    {
      name: 'node_modules/',
      reclaim: true,
      children: [{ name: '.pnpm/' }, { name: 'react/' }, { name: 'typescript/' }],
    },
    { name: 'dist/', reclaim: true, children: [{ name: 'index.js' }] },
  ],
  [
    { name: 'src/', children: [{ name: 'main.rs' }, { name: 'lib.rs' }] },
    { name: 'Cargo.toml' },
    { name: 'target/', reclaim: true, children: [{ name: 'debug/' }, { name: 'release/' }] },
  ],
  [
    { name: 'app/', children: [{ name: '__init__.py' }, { name: 'main.py' }] },
    { name: 'pyproject.toml' },
    { name: '.venv/', reclaim: true, children: [{ name: 'lib/' }, { name: 'bin/' }] },
    { name: '__pycache__/', reclaim: true },
  ],
  [
    { name: 'app/', children: [{ name: 'page.tsx' }, { name: 'layout.tsx' }] },
    { name: 'next.config.ts' },
    { name: '.next/', reclaim: true, children: [{ name: 'cache/' }, { name: 'server/' }] },
    { name: 'node_modules/', reclaim: true, children: [{ name: 'next/' }, { name: '.bin/' }] },
  ],
  [
    { name: 'lib/', children: [{ name: 'main.dart' }] },
    { name: 'pubspec.yaml' },
    { name: 'build/', reclaim: true, children: [{ name: 'app/' }, { name: 'ios/' }] },
    { name: '.dart_tool/', reclaim: true },
  ],
  [
    { name: 'cmd/', children: [{ name: 'main.go' }] },
    { name: 'go.mod' },
    { name: 'bin/', reclaim: true },
    { name: 'vendor/', children: [{ name: 'modules.txt' }] },
  ],
]

type Node = {
  id: number
  name: string
  size: string
  reclaim: boolean
  children: Node[]
  foundAt: number
}

type Row = {
  node: Node
  prefix: string
  depth: number
}

type Placed = {
  y: number
  alpha: number
}

type Column = {
  x: number
  top: number
  speed: number
  projects: Node[]
  placed: Map<number, Placed>
}

type Field = {
  width: number
  height: number
  scale: number
  charWidth: number
  line: number
  mouse: { x: number; y: number; active: boolean }
  columns: Column[]
  nextId: number
  nextProject: number
}

function pick<T>(items: T[]): T {
  return items[Math.floor(Math.random() * items.length)] as T
}

function formatSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)}G`
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)}M`
  return `${Math.max(1, Math.round(bytes / 1024))}K`
}

function randomSize(reclaim: boolean, folder: boolean): string {
  if (reclaim) return formatSize(1024 ** 2 * (40 + Math.random() * 2400))
  if (folder) return formatSize(1024 * (20 + Math.random() * 900))
  return formatSize(1024 * (1 + Math.random() * 24))
}

function build(field: Field, spec: Spec): Node {
  const children = (spec.children ?? []).filter(() => Math.random() > 0.2).map((child) => build(field, child))
  field.nextId += 1
  return {
    id: field.nextId,
    name: spec.name,
    size: randomSize(Boolean(spec.reclaim), spec.name.endsWith('/')),
    reclaim: Boolean(spec.reclaim),
    children,
    foundAt: 0,
  }
}

function makeProject(field: Field): Node {
  const name = projectNames[field.nextProject % projectNames.length] ?? 'project'
  field.nextProject += 1
  const specs = pick(stacks).filter((spec) => spec.reclaim || Math.random() > 0.15)
  return build(field, { name: `~/work/${name}`, children: specs })
}

function flatten(project: Node): Row[] {
  const rows: Row[] = [{ node: project, prefix: '', depth: 0 }]
  function walk(nodes: Node[], lead: string, depth: number) {
    nodes.forEach((node, index) => {
      const last = index === nodes.length - 1
      rows.push({ node, prefix: `${lead}${last ? '└─ ' : '├─ '}`, depth })
      walk(node.children, `${lead}${last ? '   ' : '│  '}`, depth + 1)
    })
  }
  walk(project.children, '', 1)
  return rows
}

function prune(nodes: Node[], now: number): Node[] {
  return nodes
    .filter((node) => !(node.foundAt > 0 && now - node.foundAt > foundFor + leaveFor))
    .map((node) => {
      node.children = prune(node.children, now)
      return node
    })
}

function leaving(node: Node, now: number): number {
  if (node.foundAt === 0) return 0
  return Math.min(1, Math.max(0, (now - node.foundAt - foundFor) / leaveFor))
}

function layout(field: Field) {
  const span = (columnChars + gapChars) * field.charWidth
  const count = Math.max(1, Math.ceil(field.width / span) + 1)
  const offset = (field.width - count * span + gapChars * field.charWidth) / 2
  field.columns = Array.from({ length: count }, (_, index) => ({
    x: offset + index * span,
    top: field.height * scanLine - Math.random() * field.height,
    speed: 0.8 + Math.random() * 0.4,
    projects: [],
    placed: new Map(),
  }))
}

function paint(gray: number, alpha: number, tint: number, light: boolean): string {
  const base = light ? 255 - gray : gray
  if (tint <= 0) return `rgba(${base}, ${base}, ${base}, ${alpha})`
  const [r, g, b] = brand
  const red = Math.round(base + (r - base) * tint)
  const green = Math.round(base + (g - base) * tint)
  const blue = Math.round(base + (b - base) * tint)
  return `rgba(${red}, ${green}, ${blue}, ${Math.min(1, alpha + 0.45 * tint)})`
}

function step(column: Column, field: Field, now: number, delta: number, moving: boolean) {
  const { line, height } = field
  if (moving) column.top -= (rowsPerSecond * column.speed * line * delta) / 1000
  column.projects = prune(column.projects, now)

  let rows = column.projects.flatMap(flatten)
  while (column.top + rows.length * line < height + line * 4) {
    column.projects.push(makeProject(field))
    rows = column.projects.flatMap(flatten)
  }

  const first = column.projects[0]
  if (first) {
    const firstRows = flatten(first).length + 1
    if (column.top + firstRows * line < -line * 2) {
      column.projects.shift()
      column.top += firstRows * line
      rows = column.projects.flatMap(flatten)
    }
  }

  const scan = height * scanLine
  const seen = new Set<number>()
  let index = 0
  for (const project of column.projects) {
    for (const row of flatten(project)) {
      const target = column.top + index * line
      const placed = column.placed.get(row.node.id)
      if (placed) {
        placed.y += (target - placed.y) * Math.min(1, delta / 90)
      } else {
        column.placed.set(row.node.id, { y: target, alpha: 1 })
      }
      const y = column.placed.get(row.node.id)?.y ?? target
      if (row.node.reclaim && row.node.foundAt === 0 && y <= scan && moving) row.node.foundAt = now
      seen.add(row.node.id)
      index += 1
    }
    index += 1
  }
  for (const id of column.placed.keys()) {
    if (!seen.has(id)) column.placed.delete(id)
  }
}

function inherited(node: Node, ancestors: Node[]): Node | undefined {
  if (node.foundAt > 0) return node
  return ancestors.find((ancestor) => ancestor.foundAt > 0)
}

function draw(context: CanvasRenderingContext2D, field: Field, now: number, delta: number, moving: boolean) {
  const { width, height, line, charWidth, mouse } = field
  const light = readTheme() === 'light'
  const scan = height * scanLine
  const center = width / 2
  context.clearRect(0, 0, width, height)

  for (const column of field.columns) {
    step(column, field, now, delta, moving)
    const middle = column.x + (columnChars * charWidth) / 2
    const quiet = 0.3 + 0.7 * Math.min(1, Math.abs(middle - center) / (width * 0.32))

    for (const project of column.projects) {
      const stack: Node[] = []
      for (const row of flatten(project)) {
        while (stack.length >= row.depth + 1) stack.pop()
        const ancestors = [...stack]
        stack.push(row.node)
        const y = column.placed.get(row.node.id)?.y
        if (y === undefined || y < -line || y > height + line) continue

        const owner = inherited(row.node, ancestors)
        const fade = owner ? 1 - leaving(owner, now) : 1
        const tint = owner ? Math.max(0, 1 - (now - owner.foundAt) / (foundFor + leaveFor)) : 0
        const scanned = y < scan
        const cursor = Math.abs(y - scan) < line / 2
        const hovered =
          mouse.active &&
          Math.abs(mouse.y - y) < line / 2 &&
          mouse.x >= column.x &&
          mouse.x <= column.x + columnChars * charWidth
        const folder = row.node.name.endsWith('/') || row.depth === 0
        let gray = folder ? 64 : 52
        if (cursor || hovered) gray = 92
        const alpha = (scanned ? 0.2 : 0.3) * quiet * fade * (cursor || hovered ? 1.6 : 1)

        context.fillStyle = paint(40, alpha, 0, light)
        context.fillText(row.prefix, column.x, y)
        const nameX = column.x + row.prefix.length * charWidth
        context.fillStyle = paint(gray, alpha, tint, light)
        context.fillText(row.node.name, nameX, y)

        const label = owner === row.node && tint > 0 ? `-${row.node.size}` : row.node.size
        const labelX = column.x + (columnChars - label.length) * charWidth
        if (labelX > nameX + (row.node.name.length + 1) * charWidth) {
          context.fillStyle = paint(
            owner === row.node ? gray : 44,
            alpha,
            owner === row.node ? tint : 0,
            light,
          )
          context.fillText(label, labelX, y)
        }
      }
    }
  }
}

export function HeroField() {
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const rendering = canvas.getContext('2d')
    if (!rendering) return
    const context: CanvasRenderingContext2D = rendering

    const scale = Math.min(window.devicePixelRatio || 1, 2)
    const field: Field = {
      width: 0,
      height: 0,
      scale,
      charWidth: 7 * scale,
      line: 17 * scale,
      mouse: { x: 0, y: 0, active: false },
      columns: [],
      nextId: 0,
      nextProject: Math.floor(Math.random() * projectNames.length),
    }
    let visible = true
    let frame = 0
    let last = 0

    function resize() {
      if (!canvas) return
      const rect = canvas.getBoundingClientRect()
      field.width = rect.width * scale
      field.height = rect.height * scale
      canvas.width = field.width
      canvas.height = field.height
      const mono = getComputedStyle(canvas).getPropertyValue('--font-plex-mono').trim()
      context.font = `${11 * scale}px ${mono ? `${mono}, ` : ''}ui-monospace, SFMono-Regular, Menlo, monospace`
      context.textAlign = 'left'
      context.textBaseline = 'middle'
      field.charWidth = context.measureText('M').width
      if (field.width > 0 && field.height > 0) {
        layout(field)
        draw(context, field, performance.now(), 0, false)
      }
    }

    function onMove(event: MouseEvent) {
      if (!canvas) return
      const rect = canvas.getBoundingClientRect()
      field.mouse.x = (event.clientX - rect.left) * scale
      field.mouse.y = (event.clientY - rect.top) * scale
      field.mouse.active =
        field.mouse.x >= 0 &&
        field.mouse.x <= field.width &&
        field.mouse.y >= 0 &&
        field.mouse.y <= field.height
    }

    function onLeave() {
      field.mouse.active = false
    }

    const resizeObserver = new ResizeObserver(resize)
    resizeObserver.observe(canvas)
    resize()
    const intersection = new IntersectionObserver(([entry]) => {
      visible = entry?.isIntersecting ?? true
    })
    intersection.observe(canvas)
    window.addEventListener('mousemove', onMove)
    document.addEventListener('mouseleave', onLeave)

    function tick(now: number) {
      frame = window.requestAnimationFrame(tick)
      const delta = last === 0 ? 16 : Math.min(64, now - last)
      last = now
      if (!visible || field.width === 0) return
      draw(context, field, now, delta, true)
    }
    frame = window.requestAnimationFrame(tick)

    return () => {
      window.cancelAnimationFrame(frame)
      resizeObserver.disconnect()
      intersection.disconnect()
      window.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseleave', onLeave)
    }
  }, [])

  return (
    <canvas
      ref={canvasRef}
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 -z-1 size-full animate-fade-slow mask-b-from-40% mask-b-to-65%"
    />
  )
}

'use client'

import { useEffect, useRef } from 'react'
import { readTheme } from '@/shared/helpers/theme'

const source =
  'kuizer~/projectsscan.run({depth:8,follow:false})select.all({safe:true})trash.move({items:14,bytes:"27.5GiB"})kuizerhistory--last5kuizerrestore20261017-2031cache.rebuild({tool:"cargo"})if(!entry.safe)review(entry)kuizer--gui~/work'
const boidCount = 90
const trailDecay = 0.996
const minTrailAlpha = 0.03
const brand = [210, 86, 17] as const

type Boid = {
  x: number
  y: number
  vx: number
  vy: number
  ax: number
  ay: number
  pointer: number
  stampX: number
  stampY: number
}

type Trail = {
  char: string
  gray: number
  alpha: number
}

type Ripple = {
  x: number
  y: number
  radius: number
}

type Field = {
  width: number
  height: number
  scale: number
  cell: number
  mouse: { x: number; y: number; active: boolean }
  ripples: Ripple[]
  trails: Map<string, Trail>
  boids: Boid[]
}

function makeBoid(field: Field): Boid {
  const heading = Math.random() * Math.PI * 2
  const speed = 1.2 * field.scale * (0.5 + 0.5 * Math.random())
  const x = Math.random() * field.width
  const y = Math.random() * field.height
  return {
    x,
    y,
    vx: Math.cos(heading) * speed,
    vy: Math.sin(heading) * speed,
    ax: 0,
    ay: 0,
    pointer: Math.floor(Math.random() * source.length),
    stampX: x,
    stampY: y,
  }
}

function steer(boid: Boid, dx: number, dy: number, weight: number, scale: number) {
  const distance = Math.hypot(dx, dy)
  if (distance === 0) return
  const speed = 1.2 * scale
  let fx = (dx / distance) * speed - boid.vx
  let fy = (dy / distance) * speed - boid.vy
  const force = Math.hypot(fx, fy)
  const maxForce = 0.035 * scale
  if (force > maxForce) {
    fx = (fx / force) * maxForce
    fy = (fy / force) * maxForce
  }
  boid.ax += fx * weight
  boid.ay += fy * weight
}

function flock(boid: Boid, field: Field) {
  const { scale, boids, mouse } = field
  let sepX = 0
  let sepY = 0
  let sepCount = 0
  let velX = 0
  let velY = 0
  let velCount = 0
  let posX = 0
  let posY = 0
  let posCount = 0
  for (const other of boids) {
    if (other === boid) continue
    const dx = other.x - boid.x
    const dy = other.y - boid.y
    const distance = Math.hypot(dx, dy)
    if (distance > 90 * scale || distance === 0) continue
    if (distance < 44 * scale) {
      sepX -= dx / distance / distance
      sepY -= dy / distance / distance
      sepCount += 1
    }
    velX += other.vx
    velY += other.vy
    velCount += 1
    posX += other.x
    posY += other.y
    posCount += 1
  }
  if (sepCount > 0) steer(boid, sepX, sepY, 1.7, scale)
  if (velCount > 0) steer(boid, velX / velCount, velY / velCount, 1, scale)
  if (posCount > 0) steer(boid, posX / posCount - boid.x, posY / posCount - boid.y, 0.45, scale)
  if (mouse.active) {
    const dx = mouse.x - boid.x
    const dy = mouse.y - boid.y
    const distance = Math.hypot(dx, dy)
    const reach = 280 * scale
    steer(boid, dx, dy, distance < reach ? (distance / reach) * 0.5 : 0.5, scale)
  }
}

function pickGray(): number {
  const roll = Math.random()
  if (roll < 0.7) return 45
  if (roll < 0.92) return 56
  return 68
}

function move(boid: Boid, field: Field) {
  const { scale, cell, width, height, trails } = field
  boid.vx += boid.ax
  boid.vy += boid.ay
  const speed = Math.hypot(boid.vx, boid.vy)
  const maxSpeed = 1.2 * scale
  if (speed > maxSpeed) {
    boid.vx = (boid.vx / speed) * maxSpeed
    boid.vy = (boid.vy / speed) * maxSpeed
  }
  boid.x += boid.vx
  boid.y += boid.vy
  boid.ax = 0
  boid.ay = 0

  let wrapped = false
  if (boid.x < -cell) {
    boid.x = width + cell
    wrapped = true
  } else if (boid.x > width + cell) {
    boid.x = -cell
    wrapped = true
  }
  if (boid.y < -cell) {
    boid.y = height + cell
    wrapped = true
  } else if (boid.y > height + cell) {
    boid.y = -cell
    wrapped = true
  }

  if (wrapped) {
    boid.stampX = boid.x
    boid.stampY = boid.y
    return
  }
  if (Math.hypot(boid.x - boid.stampX, boid.y - boid.stampY) < 1.5 * cell) return
  boid.stampX = boid.x
  boid.stampY = boid.y
  const char = source.charAt(boid.pointer % source.length)
  boid.pointer += 1
  const column = Math.round(boid.x / cell)
  const row = Math.round(boid.y / cell)
  trails.set(`${column},${row}`, { char, gray: pickGray(), alpha: 0.22 + 0.1 * Math.random() })
}

function rippleStrength(field: Field, x: number, y: number): number {
  let strength = 0
  for (const ripple of field.ripples) {
    const onRing = 1 - Math.abs(Math.hypot(x - ripple.x, y - ripple.y) - ripple.radius) / (110 * field.scale)
    if (onRing > strength) strength = onRing
  }
  return strength
}

function paint(gray: number, alpha: number, tint: number, light: boolean): string {
  const base = light ? 255 - gray : gray
  if (tint <= 0) return `rgba(${base}, ${base}, ${base}, ${alpha})`
  const [r, g, b] = brand
  const red = Math.round(base + (r - base) * tint)
  const green = Math.round(base + (g - base) * tint)
  const blue = Math.round(base + (b - base) * tint)
  return `rgba(${red}, ${green}, ${blue}, ${Math.min(1, alpha + 0.3 * tint)})`
}

function draw(context: CanvasRenderingContext2D, field: Field) {
  const { width, height, scale, cell, ripples, trails, boids } = field
  const light = readTheme() === 'light'
  context.clearRect(0, 0, width, height)

  for (let index = ripples.length - 1; index >= 0; index -= 1) {
    const ripple = ripples[index]
    if (!ripple) continue
    ripple.radius += 7 * scale
    if (ripple.radius - 110 * scale > Math.hypot(width, height)) ripples.splice(index, 1)
  }

  for (const [key, trail] of trails) {
    trail.alpha *= trailDecay
    if (trail.alpha < minTrailAlpha) {
      trails.delete(key)
      continue
    }
    const [column, row] = key.split(',')
    const x = Number(column) * cell
    const y = Number(row) * cell
    context.fillStyle = paint(trail.gray, trail.alpha, rippleStrength(field, x, y), light)
    context.fillText(trail.char, x, y)
  }

  for (const boid of boids) flock(boid, field)
  for (const boid of boids) {
    move(boid, field)
    const x = Math.round(boid.x / cell) * cell
    const y = Math.round(boid.y / cell) * cell
    const head = source.charAt(boid.pointer % source.length)
    context.fillStyle = paint(52, 0.35, rippleStrength(field, x, y), light)
    context.fillText(head, x, y)
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
      cell: 13 * scale,
      mouse: { x: 0, y: 0, active: false },
      ripples: [],
      trails: new Map(),
      boids: [],
    }
    let visible = true
    let frame = 0

    function resize() {
      if (!canvas) return
      const rect = canvas.getBoundingClientRect()
      field.width = rect.width * scale
      field.height = rect.height * scale
      canvas.width = field.width
      canvas.height = field.height
      const mono = getComputedStyle(canvas).getPropertyValue('--font-plex-mono').trim()
      context.font = `${11 * scale}px ${mono ? `${mono}, ` : ''}ui-monospace, SFMono-Regular, Menlo, monospace`
      context.textAlign = 'center'
      context.textBaseline = 'middle'
      if (field.boids.length === 0 && field.width > 0 && field.height > 0) {
        field.boids = Array.from({ length: boidCount }, () => makeBoid(field))
      }
    }

    function toField(event: MouseEvent) {
      if (!canvas) return null
      const rect = canvas.getBoundingClientRect()
      return { x: (event.clientX - rect.left) * scale, y: (event.clientY - rect.top) * scale }
    }

    function inside(point: { x: number; y: number }) {
      return point.x >= 0 && point.x <= field.width && point.y >= 0 && point.y <= field.height
    }

    function onMove(event: MouseEvent) {
      const point = toField(event)
      if (!point) return
      field.mouse.x = point.x
      field.mouse.y = point.y
      field.mouse.active = inside(point)
    }

    function onDown(event: MouseEvent) {
      const point = toField(event)
      if (point && inside(point)) field.ripples.push({ x: point.x, y: point.y, radius: 0 })
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
    window.addEventListener('mousedown', onDown)
    document.addEventListener('mouseleave', onLeave)

    function tick() {
      frame = window.requestAnimationFrame(tick)
      if (!visible || field.width === 0) return
      draw(context, field)
    }
    frame = window.requestAnimationFrame(tick)

    return () => {
      window.cancelAnimationFrame(frame)
      resizeObserver.disconnect()
      intersection.disconnect()
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mousedown', onDown)
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

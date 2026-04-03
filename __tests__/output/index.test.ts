import { describe, expect, it } from 'bun:test'
import { badge, indent, rule } from '../../src/output/index.js'

describe('output', () => {
  it('rule returns a string of the given width', () => {
    const raw = rule(20).replace(/\x1b\[[0-9;]*m/g, '')
    expect(raw).toHaveLength(20)
  })

  it('indent prepends spaces to each line', () => {
    expect(indent('a\nb', 2)).toBe('  a\n  b')
  })

  it('indent respects custom depth', () => {
    expect(indent('x', 4)).toBe('    x')
  })

  it('badge pads severity to 5 characters', () => {
    const raw = badge('error').replace(/\x1b\[[0-9;]*m/g, '')
    expect(raw.trim()).toBe('error')
    expect(raw).toHaveLength(5)
  })

  it('badge pads short severity labels', () => {
    const raw = badge('warn').replace(/\x1b\[[0-9;]*m/g, '')
    expect(raw).toHaveLength(5)
    expect(raw.trim()).toBe('warn')
  })

  it('badge includes the severity label for each supported level', () => {
    expect(badge('error')).toContain('error')
    expect(badge('warn')).toContain('warn')
    expect(badge('info')).toContain('info')
  })
})

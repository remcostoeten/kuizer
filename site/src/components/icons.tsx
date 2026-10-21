import Image from 'next/image'
import type { Feature } from '@/content/features'

type IconProps = {
  size?: number
}

function Svg({ size = 16, children }: IconProps & { children: React.ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  )
}

export function Logo({ size = 22 }: IconProps) {
  return <Image src="/brand/kuizer.webp" alt="" width={size} height={size} className="shrink-0" />
}

export function Wordmark() {
  return (
    <span className="inline-flex items-center gap-2.5">
      <Logo size={40} />
      <span className="font-mono text-label font-semibold uppercase tracking-brand">kuizer</span>
    </span>
  )
}

export function DownloadIcon({ size = 14 }: IconProps) {
  return (
    <Svg size={size}>
      <path d="M8 2.5v8M4.5 7.5 8 11l3.5-3.5M2.5 13.5h11" />
    </Svg>
  )
}

export function StarIcon({ size = 14 }: IconProps) {
  return (
    <Svg size={size}>
      <path d="m8 1.8 1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.6l-3.8 2 .7-4.3-3.1-3 4.3-.6z" />
    </Svg>
  )
}

export function ArrowRightIcon({ size = 12 }: IconProps) {
  return (
    <Svg size={size}>
      <path d="M3 8h10M9 4l4 4-4 4" />
    </Svg>
  )
}

export function ArrowDownIcon({ size = 16 }: IconProps) {
  return (
    <Svg size={size}>
      <path d="M8 3v10M4 9l4 4 4-4" />
    </Svg>
  )
}

export function GithubIcon({ size = 16 }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z" />
    </svg>
  )
}

export function FeatureIcon({ name, size = 16 }: IconProps & { name: Feature['icon'] }) {
  switch (name) {
    case 'check':
      return (
        <Svg size={size}>
          <circle cx="8" cy="8" r="6.5" />
          <path d="m5 8.2 2 2 4-4.5" />
        </Svg>
      )
    case 'trash':
      return (
        <Svg size={size}>
          <path d="M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5M3.5 4.5l.7 8.2a1 1 0 0 0 1 .8h5.6a1 1 0 0 0 1-.8l.7-8.2" />
        </Svg>
      )
    case 'shield':
      return (
        <Svg size={size}>
          <path d="M8 1.5 13 3.5v4c0 3.2-2.1 5.3-5 6.5-2.9-1.2-5-3.3-5-6.5v-4z" />
        </Svg>
      )
    case 'git':
      return (
        <Svg size={size}>
          <circle cx="4.5" cy="3.5" r="1.5" />
          <circle cx="4.5" cy="12.5" r="1.5" />
          <circle cx="11.5" cy="5.5" r="1.5" />
          <path d="M4.5 5v6M11.5 7a4 4 0 0 1-4 4h-1" />
        </Svg>
      )
    case 'recheck':
      return (
        <Svg size={size}>
          <path d="M13.5 8A5.5 5.5 0 1 1 11.8 4M13.5 2v3h-3" />
        </Svg>
      )
    case 'history':
      return (
        <Svg size={size}>
          <circle cx="8" cy="8" r="6.5" />
          <path d="M8 4.5V8l2.5 1.5" />
        </Svg>
      )
  }
}

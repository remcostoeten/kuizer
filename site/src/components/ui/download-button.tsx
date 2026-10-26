'use client'

import { useSyncExternalStore } from 'react'
import { DownloadIcon } from '@/components/ui/icons'
import { latestReleaseUrl } from '@/content/site'
import { readPlatform, readServerPlatform, subscribePlatform } from '@/shared/helpers/platform'

type Props = {
  size?: 'md' | 'sm'
  short?: boolean
}

export function DownloadButton({ size = 'md', short = false }: Props) {
  const platform = useSyncExternalStore(subscribePlatform, readPlatform, readServerPlatform)
  const label = short ? 'Download' : platform ? `Download for ${platform}` : 'Download for Linux / macOS'

  return (
    <a href={latestReleaseUrl} className={`btn btn-primary ${size === 'sm' ? 'btn-sm' : ''}`}>
      {label}
      <DownloadIcon size={size === 'sm' ? 12 : 16} />
    </a>
  )
}

import { DocsLayout } from 'fumadocs-ui/layouts/docs'
import type { ReactNode } from 'react'
import { source } from '@/modules/docs/server/source'
import { DocsSidebar, Topbar } from '@/modules/navigation'
import { docsSite } from '@/shared/config/docs-site'

type Props = {
  children: ReactNode
}

export default function Layout({ children }: Props) {
  const tree = source.getPageTree()

  return (
    <>
      <Topbar />
      <DocsSidebar tree={tree} repositoryUrl={docsSite.repository.url} />
      <DocsLayout
        tree={tree}
        nav={{ enabled: false }}
        searchToggle={{ enabled: false }}
        themeSwitch={{ enabled: false }}
        sidebar={{ enabled: false }}
        containerProps={{ className: 'docs-layout' }}
      >
        {children}
      </DocsLayout>
    </>
  )
}

import { authorUrl, description, repoUrl, siteUrl } from '@/content/site'

export const docsSite = {
  name: 'kuizer',
  title: 'kuizer documentation',
  description,
  url: siteUrl,
  basePath: '/docs',
  author: { name: 'Remco Stoeten', url: authorUrl },
  keywords: [
    'kuizer',
    'disk usage',
    'disk cleaner',
    'node_modules cleaner',
    'cargo target cleaner',
    'developer cache cleanup',
    'terminal UI',
  ],
  repository: { url: repoUrl, branch: 'master', contentDir: 'site/content/docs' },
}

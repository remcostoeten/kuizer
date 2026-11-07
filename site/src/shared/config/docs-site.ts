import { authorName, authorUrl, description, keywords, repoUrl, siteUrl } from '@/content/site'

export const docsSite = {
  name: 'kuizer',
  title: 'kuizer documentation',
  description,
  url: siteUrl,
  basePath: '/docs',
  author: { name: authorName, url: authorUrl },
  keywords,
  repository: { url: repoUrl, branch: 'master', contentDir: 'site/content/docs' },
}

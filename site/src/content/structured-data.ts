import {
  authorName,
  authorUrl,
  cratesUrl,
  githubProfileUrl,
  latestReleaseUrl,
  metaDescription,
  npmUrl,
  releasesUrl,
  repoUrl,
  siteName,
  siteUrl,
  version,
} from '@/content/site'

const ids = {
  website: `${siteUrl}/#website`,
  organization: `${siteUrl}/#organization`,
  author: `${siteUrl}/#author`,
  software: `${siteUrl}/#software`,
}

const features = [
  'Finds node_modules, .next, Cargo target, .venv and other generated folders under your projects, with sizes',
  'Checks each folder against its project: a dist folder counts only next to a package.json',
  'Lists global tool caches for npm, pnpm, yarn, bun, cargo, pip, uv, Go, Gradle, Maven and Playwright',
  'Lists installed toolchain versions from rustup, nvm, fnm, volta, pyenv, uv, SDKMAN and Go',
  'Lists Docker and Podman leftovers and git repositories that git gc would shrink',
  'Finds duplicate files by size and BLAKE3 hash',
  'Moves deletions to the trash, logs every run and restores a run with kuizer restore',
  'Runs as a terminal UI and as a desktop app that share one config file and one history',
]

const screenshots = ['overview', 'artifacts', 'light'].map((name) => ({
  '@type': 'ImageObject',
  url: `${siteUrl}/screenshots/${name}.png`,
}))

/**
 * @name softwareJsonLd
 * @description Schema.org data for the landing page: the site, the project as
 * an organization, its author and the application itself, safe to inline in a
 * script tag.
 *
 * @example
 * <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: softwareJsonLd() }} />
 */
export function softwareJsonLd() {
  const data = {
    '@context': 'https://schema.org',
    '@graph': [
      {
        '@type': 'WebSite',
        '@id': ids.website,
        url: siteUrl,
        name: siteName,
        alternateName: 'kuizer.app',
        description: metaDescription,
        inLanguage: 'en',
        publisher: { '@id': ids.organization },
      },
      {
        '@type': 'Organization',
        '@id': ids.organization,
        name: siteName,
        url: siteUrl,
        logo: {
          '@type': 'ImageObject',
          url: `${siteUrl}/brand/sweep/icons/app-512.png`,
          width: 512,
          height: 512,
        },
        founder: { '@id': ids.author },
        sameAs: [repoUrl, npmUrl, cratesUrl],
      },
      {
        '@type': 'Person',
        '@id': ids.author,
        name: authorName,
        url: authorUrl,
        sameAs: [githubProfileUrl],
      },
      {
        '@type': 'SoftwareApplication',
        '@id': ids.software,
        name: siteName,
        url: siteUrl,
        description: metaDescription,
        applicationCategory: 'DeveloperApplication',
        applicationSubCategory: 'Disk cleanup',
        operatingSystem: 'Linux, macOS',
        softwareVersion: version,
        downloadUrl: latestReleaseUrl,
        installUrl: `${siteUrl}/docs/installation`,
        softwareHelp: { '@type': 'CreativeWork', url: `${siteUrl}/docs` },
        releaseNotes: releasesUrl,
        license: 'https://opensource.org/licenses/MIT',
        isAccessibleForFree: true,
        offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
        image: `${siteUrl}/brand/sweep/social/opengraph-dark.png`,
        screenshot: screenshots,
        featureList: features,
        author: { '@id': ids.author },
        publisher: { '@id': ids.organization },
        sameAs: [repoUrl, npmUrl, cratesUrl],
      },
    ],
  }
  return JSON.stringify(data).replace(/</g, '\\u003c')
}

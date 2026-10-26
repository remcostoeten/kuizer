import {
  authorUrl,
  cratesUrl,
  latestReleaseUrl,
  metaDescription,
  npmUrl,
  repoUrl,
  siteUrl,
  version,
} from '@/content/site'

/**
 * @name softwareJsonLd
 * @description Schema.org SoftwareApplication data for the landing page, safe to
 * inline in a script tag.
 *
 * @example
 * <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: softwareJsonLd() }} />
 */
export function softwareJsonLd() {
  const data = {
    '@context': 'https://schema.org',
    '@type': 'SoftwareApplication',
    name: 'kuizer',
    url: siteUrl,
    description: metaDescription,
    applicationCategory: 'DeveloperApplication',
    operatingSystem: 'Linux, macOS',
    softwareVersion: version,
    downloadUrl: latestReleaseUrl,
    license: 'https://opensource.org/licenses/MIT',
    offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
    author: { '@type': 'Person', name: 'Remco Stoeten', url: authorUrl },
    sameAs: [repoUrl, npmUrl, cratesUrl],
  }
  return JSON.stringify(data).replace(/</g, '\\u003c')
}

import type { Metadata } from 'next'
import { DownloadButton } from '@/components/ui/download-button'
import { Footer } from '@/components/layout/footer'
import { HeroField } from '@/components/landing/hero-field'
import { GithubIcon, Logo } from '@/components/ui/icons'
import { InstallBox } from '@/components/landing/install-box'
import { Nav } from '@/components/layout/nav'
import { Playground } from '@/components/playground/playground'
import { ReleasePill } from '@/components/landing/release-pill'
import { RunLine } from '@/components/landing/run-line'
import { DeletedWord } from '@/components/ui/deleted-word'
import { principles } from '@/content/features'
import { headline, headlineTargets, repoUrl, subtitle } from '@/content/site'
import { softwareJsonLd } from '@/content/structured-data'

export const metadata: Metadata = {
  alternates: { canonical: '/' },
}

export default function HomePage() {
  return (
    <>
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: softwareJsonLd() }} />
      <Nav />
      <main>
        <section className="relative isolate overflow-x-clip">
          <HeroField />
          <div className="wrap flex flex-col items-center pt-10 text-center md:pt-14">
            <div className="mb-5">
              <Logo size={104} />
            </div>
            <ReleasePill />
            <h1
              className="enter mt-10 font-display text-hero font-medium sm:text-hero-wide md:text-hero-xl"
              style={{ '--delay': '120ms' } as React.CSSProperties}
            >
              {headline[0]}
              <br />
              {headline[1]} <DeletedWord items={headlineTargets} interval={4000} className="hero-target" />
            </h1>
            <p
              className="enter mt-6 max-w-lead text-lead font-light text-muted md:text-lead-lg"
              style={{ '--delay': '180ms' } as React.CSSProperties}
            >
              {subtitle.map((line) => (
                <span key={line} className="sm:block">
                  {line}{' '}
                </span>
              ))}
            </p>
            <div
              className="enter mt-8 flex flex-wrap items-center justify-center gap-2"
              style={{ '--delay': '240ms' } as React.CSSProperties}
            >
              <DownloadButton />
              <a href={repoUrl} aria-label="kuizer on GitHub" className="btn btn-secondary btn-icon">
                <GithubIcon size={18} />
              </a>
            </div>
            <div className="enter mt-5" style={{ '--delay': '300ms' } as React.CSSProperties}>
              <RunLine />
            </div>
          </div>
          <div className="wrap mt-16 md:mt-20">
            <div className="relative mx-auto w-full max-w-hero">
              <div className="hero-glow pointer-events-none absolute -inset-x-1/4 -top-[30%] bottom-0 -z-1" />
              <div className="enter-frame flex h-playground w-full flex-col md:h-playground-wide">
                <Playground />
              </div>
            </div>
            <p
              className="enter mx-auto mt-5 max-w-copy text-center text-caption text-faint"
              style={{ '--delay': '500ms' } as React.CSSProperties}
            >
              This is the real key map of the interactive list. Try <kbd className="key-chip">space</kbd>,{' '}
              <kbd className="key-chip">a</kbd>, <kbd className="key-chip">enter</kbd>, then{' '}
              <kbd className="key-chip">q</kbd> for a shell with <code>kuizer history</code> and{' '}
              <code>kuizer restore</code>. Nothing on this page touches your disk.
            </p>
          </div>
        </section>

        <section className="wrap flex flex-col items-center pb-28 pt-24 md:pb-36 md:pt-32">
          <ul className="grid w-full max-w-hero gap-10 text-left md:grid-cols-3 md:gap-8">
            {principles.map((item) => (
              <li key={item.title} className="border-t border-line pt-5">
                <h2 className="text-card font-medium">{item.title}</h2>
                <p className="mt-2 text-feature text-muted">{item.body}</p>
              </li>
            ))}
          </ul>
          <div className="mt-20 flex w-full justify-center">
            <InstallBox />
          </div>
        </section>
      </main>
      <Footer />
    </>
  )
}

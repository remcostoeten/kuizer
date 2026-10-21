import { DownloadButton } from '@/components/download-button'
import { Faq } from '@/components/faq'
import { Footer } from '@/components/footer'
import { HeroField } from '@/components/hero-field'
import { Highlights } from '@/components/highlights'
import { ArrowDownIcon, ArrowRightIcon, FeatureIcon, GithubIcon, Logo, StarIcon } from '@/components/icons'
import { InstallBox } from '@/components/install-box'
import { Nav } from '@/components/nav'
import { Playground } from '@/components/playground/playground'
import { RunLine } from '@/components/run-line'
import { RotatingWord } from '@/components/swap'
import { Window } from '@/components/window'
import { safeguards, showcases } from '@/content/features'
import { headline, headlineTargets, releasesUrl, repoUrl, subtitle, version } from '@/content/site'

export default function HomePage() {
  return (
    <>
      <Nav />
      <main>
        <section className="relative isolate overflow-x-clip">
          <HeroField />
          <div className="wrap flex flex-col items-center pt-10 text-center md:pt-14">
            <div className="mb-5">
              <Logo size={104} />
            </div>
            <a href={releasesUrl} className="enter pill" style={{ '--delay': '0ms' } as React.CSSProperties}>
              <span className="size-1.5 rounded-dot bg-brand" />v{version} is on npm and crates.io
              <ArrowRightIcon />
            </a>
            <h1
              className="enter mt-10 font-display text-hero font-medium sm:text-hero-wide md:text-hero-xl"
              style={{ '--delay': '120ms' } as React.CSSProperties}
            >
              {headline[0]}
              <br />
              <span className="bracket relative inline-block px-[0.18em]">
                {headline[1]} <RotatingWord items={headlineTargets} interval={3400} />
              </span>
            </h1>
            <p
              className="enter mt-6 max-w-lead text-lead font-light text-muted md:text-lead-lg"
              style={{ '--delay': '180ms' } as React.CSSProperties}
            >
              {subtitle}
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

        <Highlights />

        <section className="wrap flex flex-col items-center pt-28 text-center md:pt-36">
          <span className="text-faint">
            <ArrowDownIcon />
          </span>
          <h2 className="mt-6 text-section font-normal md:text-section-lg">The same scan, in a window</h2>
          <p className="mt-3 max-w-reading text-body text-muted">
            kuizer-gui shows the overview, project artifacts, global caches, duplicates and history, and moves
            what you tick to the trash with the same checks as the terminal.
          </p>
          <Window
            title="kuizer-gui ~/projects"
            src="/screenshots/overview.png"
            alt="The desktop app overview with totals per category, the largest projects and the largest caches"
            className="mt-12 w-full max-w-hero"
          />
        </section>

        <section className="wrap flex flex-col gap-28 pt-32 md:gap-40 md:pt-44">
          {showcases.map((item, index) => (
            <article key={item.id} className="grid items-center gap-10 lg:showcase-grid lg:gap-20">
              <div className={index % 2 === 1 ? 'lg:order-2' : ''}>
                <span className="font-mono text-sm font-medium tracking-widest uppercase text-brand">
                  {item.eyebrow}
                </span>
                <h2 className="mt-3 text-section font-medium md:text-section-lg">{item.title}</h2>
                <p className="mt-4 max-w-feature text-copy text-muted">{item.body}</p>
              </div>
              <Window
                title={item.windowTitle}
                src={item.image}
                alt={item.alt}
                className={index % 2 === 1 ? 'lg:order-1' : ''}
              />
            </article>
          ))}
        </section>

        <section className="wrap pt-32 md:pt-44">
          <span className="font-mono text-sm font-medium tracking-widest uppercase text-brand">Safety</span>
          <h2 className="mt-3 text-display font-medium md:text-display-lg">
            Safe by default.
            <br />
            Nothing is gone for good.
          </h2>
          <p className="mt-5 max-w-reading text-body text-muted">
            A scan never deletes anything, and a deletion never skips the trash.
          </p>
          <ul className="mt-12 grid gap-4 md:grid-cols-3">
            {safeguards.map((feature) => (
              <li key={feature.title} className="panel p-6">
                <span className="inline-flex size-8 items-center justify-center rounded-panel border border-line text-muted">
                  <FeatureIcon name={feature.icon} size={14} />
                </span>
                <h3 className="mt-5 text-card font-medium">{feature.title}</h3>
                <p className="mt-2 text-feature text-faint">{feature.body}</p>
              </li>
            ))}
          </ul>
        </section>

        <section className="wrap grid gap-10 pt-32 md:pt-44 lg:faq-grid lg:gap-20">
          <h2 className="text-display font-medium md:text-display-lg">
            Frequently
            <br />
            asked questions
          </h2>
          <Faq />
        </section>

        <section className="wrap flex flex-col items-center pb-28 pt-32 text-center md:pb-36 md:pt-44">
          <h2 className="text-display-plain font-medium md:text-display-plain-lg">Reclaim your disk.</h2>
          <div className="mt-8 flex flex-wrap items-center justify-center gap-2">
            <DownloadButton />
            <a href={repoUrl} className="btn btn-secondary">
              Star on GitHub
              <StarIcon />
            </a>
          </div>
          <p className="mt-10 text-label text-faint">
            Or install it with the package manager you already have.
          </p>
          <div className="mt-4 flex w-full justify-center">
            <InstallBox />
          </div>
        </section>
      </main>
      <Footer />
    </>
  )
}

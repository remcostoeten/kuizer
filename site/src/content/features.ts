export type Principle = {
  title: string
  body: string
}

export const principles: Principle[] = [
  {
    title: 'Scanning never deletes',
    body: 'Nothing leaves the disk until you tick it and confirm.',
  },
  {
    title: 'Confirmed and logged',
    body: 'A confirmation shows the item count and total size, and every run is logged. Deletion is permanent unless you turn on trash mode.',
  },
  {
    title: 'Checked against the project',
    body: 'A dist folder counts only next to a package.json.',
  },
]

export type Finding = {
  title: string
  body: string
}

export const finds: Finding[] = [
  {
    title: 'Project folders',
    body: 'node_modules, .next, .nuxt, .svelte-kit, dist, build, coverage, Cargo target, .venv, __pycache__, .pytest_cache, .gradle, bin and obj, Pods, _build and deps, zig-out, .stack-work, .terraform and the Unity Library, each checked against the marker files of its project.',
  },
  {
    title: 'Global caches',
    body: 'The npm, pnpm, yarn, bun, deno, cargo, pip, uv, Go, Gradle, Maven and Playwright caches, among others.',
  },
  {
    title: 'Toolchains',
    body: 'rustup toolchains, Node versions from nvm, fnm and volta, Python versions from pyenv and uv, SDKMAN candidates and Go SDKs. A version a project pins is marked review.',
  },
  {
    title: 'Docker and Podman',
    body: 'Dangling images, stopped containers, the build cache and volumes no container uses.',
  },
  {
    title: 'Git repositories',
    body: 'Loose objects that git gc would pack, and local branches already merged into the default branch.',
  },
  {
    title: 'Duplicate files',
    body: 'Files with identical contents, found by size and BLAKE3 hash. They are reported and never deleted.',
  },
]

export const neverTouches = [
  'Your home directory itself, system directories, mount points and symlinks.',
  'Browser and app profiles: Chrome, Brave, Firefox, Edge and Electron apps such as Discord, Slack and VS Code.',
  '~/.ssh, ~/.gnupg and the keyrings.',
  'Globally installed tools and editor extensions. Anything else in a hidden home folder is review, never safe.',
  'Anything you did not select. Deletion is permanent by default, every run is logged, and trash mode is optional.',
]

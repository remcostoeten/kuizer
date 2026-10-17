export type InstallOption = {
  id: string
  label: string
  command: string
  note: string
}

export const installOptions: InstallOption[] = [
  {
    id: 'npx',
    label: 'npx',
    command: 'npx kuizer',
    note: 'Runs the prebuilt binary without installing. Needs Node.',
  },
  {
    id: 'npm',
    label: 'npm',
    command: 'npm install -g kuizer',
    note: 'Keeps the binary on your PATH through npm.',
  },
  {
    id: 'curl',
    label: 'curl',
    command:
      'curl -LsSf https://github.com/remcostoeten/kuizer/releases/latest/download/kuizer-installer.sh | sh',
    note: 'The cargo-dist installer. Picks the archive for your platform and installs into ~/.cargo/bin.',
  },
  {
    id: 'binstall',
    label: 'cargo binstall',
    command: 'cargo binstall kuizer',
    note: 'The prebuilt binary through cargo-binstall.',
  },
  {
    id: 'cargo',
    label: 'cargo install',
    command: 'cargo install kuizer',
    note: 'Compiles from crates.io. Needs Rust 1.88 or newer.',
  },
]

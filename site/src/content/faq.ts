export type Question = {
  question: string
  answer: string
}

export const questions: Question[] = [
  {
    question: 'Does a scan delete anything?',
    answer:
      'No. kuizer, kuizer scan and kuizer dashboard only list what they find. Deletion happens in the interactive list or through kuizer clean, and always after a confirmation.',
  },
  {
    question: 'What is the difference between safe and review?',
    answer:
      'Safe means the folder sits next to the file that produces it, such as node_modules next to a package.json, so the next install or build recreates it. Review is everything kuizer cannot verify that way, including anything inside a hidden folder of your home directory.',
  },
  {
    question: 'Where do deleted folders go?',
    answer:
      'To the system trash. Every run is logged to ~/.kuizer-history.json, and kuizer restore puts a run back until you empty the trash.',
  },
  {
    question: 'Which platforms does it run on?',
    answer:
      'Prebuilt binaries exist for Linux and macOS on x86_64 and ARM. cargo install builds it anywhere Rust 1.88 or newer runs.',
  },
  {
    question: 'Is there a graphical version?',
    answer:
      'Yes. kuizer-gui is a desktop window built on GPUI with the same scan and the same checks. It installs through the cargo-dist installer or with cargo install kuizer-gui.',
  },
  {
    question: 'How do I keep a folder out of the scan?',
    answer:
      'Put a .kuizerignore file in it. That folder and everything under it is skipped. Other settings live in ~/.config/kuizer/config.toml, and every key there is optional.',
  },
  {
    question: 'Can I script it?',
    answer:
      'Every command prints JSON with --json, kuizer clean --dry-run shows what a run would remove, and kuizer scan --report writes a self-contained HTML treemap.',
  },
  {
    question: 'What happened to sweep?',
    answer:
      'kuizer was called sweep until October 2026. It still reads .sweepignore markers and ~/.sweep-history.json.',
  },
]

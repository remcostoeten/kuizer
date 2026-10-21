import type { Metadata } from 'next'
import LogoLab from './logo-lab'
import './logo-lab.css'

export const metadata: Metadata = {
  title: 'Logo lab',
  description: 'Four material studies for the Kuizer app icon.',
  robots: { index: false, follow: false },
}

export default function LogoLabPage() {
  return <LogoLab />
}

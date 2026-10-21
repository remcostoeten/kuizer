import type { Metadata } from 'next'
import { LogoStudio } from '@/components/logo-studio/logo-studio'
import './logos.css'

export const metadata: Metadata = {
  title: 'Logo explorations',
  description: 'Original Kuizer app icon concepts, exploring character, geometry and material.',
  robots: { index: false, follow: false },
}

export default function LogosPage() {
  return <LogoStudio />
}

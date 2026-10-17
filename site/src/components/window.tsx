import Image from 'next/image'

type Props = {
  title: string
  src: string
  alt: string
  priority?: boolean
  className?: string
}

export function Window({ title, src, alt, priority = false, className = '' }: Props) {
  return (
    <figure className={`window ${className}`}>
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <span className="flex gap-1.5">
          <i className="size-2.5 rounded-full bg-traffic-red" />
          <i className="size-2.5 rounded-full bg-traffic-yellow" />
          <i className="size-2.5 rounded-full bg-traffic-green" />
        </span>
        <figcaption className="ml-2 truncate font-mono text-micro text-faint">{title}</figcaption>
      </div>
      <Image src={src} alt={alt} width={1600} height={1025} priority={priority} className="block w-full" />
    </figure>
  )
}

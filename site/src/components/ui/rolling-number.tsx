type Props = {
  value: string
  className?: string
}

const digits = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'] as const

function isDigit(char: string) {
  return char >= '0' && char <= '9'
}

export function RollingNumber({ value, className = '' }: Props) {
  let column = 0
  return (
    <span className={`inline-flex whitespace-nowrap tabular-nums ${className}`}>
      <span className="sr-only">{value}</span>
      {Array.from(value).map((char, position) => {
        if (!isDigit(char)) {
          return (
            <span key={position} aria-hidden="true">
              {char}
            </span>
          )
        }
        const delay = column * 60
        column += 1
        return (
          <span key={position} aria-hidden="true" className="roll-window inline-flex">
            <span
              className="roll-strip"
              style={{ '--digit': Number(char), '--delay': `${delay}ms` } as React.CSSProperties}
            >
              {digits.map((digit) => (
                <span key={digit}>{digit}</span>
              ))}
            </span>
          </span>
        )
      })}
    </span>
  )
}

export type ButtonVariant = 'primary' | 'secondary' | 'ghost'

export type ButtonProps = {
  label: string
  variant?: ButtonVariant
  disabled?: boolean
}

export function renderButton(props: ButtonProps): string {
  return `<button class="${props.variant ?? 'primary'}">${props.label}</button>`
}

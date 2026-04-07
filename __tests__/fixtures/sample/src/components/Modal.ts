export type ModalProps = {
  title:   string
  content: string
}

export function renderModal(props: ModalProps): string {
  return `<dialog><h2>${props.title}</h2><p>${props.content}</p></dialog>`
}

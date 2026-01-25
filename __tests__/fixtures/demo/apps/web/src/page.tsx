interface PageProps {
  slug: string;
}

export function Page(props: PageProps) {
  return <main>{props.slug}</main>;
}

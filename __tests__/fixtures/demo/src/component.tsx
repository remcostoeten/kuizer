type SomeProps = {
  title: string;
};

export function Component(props: SomeProps) {
  return <div>{props.title}</div>;
}

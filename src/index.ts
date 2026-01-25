export type TAct =
  | "rm-decl"
  | "unexport"
  | "keep"
  | "keep-local"
  | "bad-nonexport-external"
  | "skip-merged";

export type TRes = {
  file: string;
  name: string;
  kind: "type" | "iface";
  line: number;
  act: TAct;
  barrels: string[];
};

export type TSet = {
  root: string;
  tsCfg: string;
  src: string;
  bars: string[];
};

export function analyze(set: TSet): TRes[] {
  void set;
  return [];
}

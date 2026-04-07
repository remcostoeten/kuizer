export type ID = string

export type Result<T> =
  | { ok: true;  value: T }
  | { ok: false; error: string }

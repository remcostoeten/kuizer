export type ID = string

export type Timestamp = number

export type Result<T> =
  | { ok: true;  value: T }
  | { ok: false; error: string }

export type PaginatedResponse<T> = {
  items:   T[]
  total:   number
  page:    number
  perPage: number
}

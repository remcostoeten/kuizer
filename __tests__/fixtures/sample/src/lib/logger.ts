export class Logger {
  constructor(private readonly name: string) {}

  info(msg: string) {
    console.log(`[${this.name}] ${msg}`)
  }
}

// Unused exports - should be flagged by unusedExports detector
export class FileLogger {
	log() {}
}

export interface PaginatedResponse<T> {
	data: T[]
	total: number
}

export type Timestamp = number

export function shout(msg: string): string {
	return msg.toUpperCase()
}

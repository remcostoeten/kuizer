export class Logger {
  constructor(private readonly name: string) {}

  info(msg: string) {
    console.log(`[${this.name}] ${msg}`)
  }
}

export class FileLogger {
  constructor(private readonly path: string) {}

  write(msg: string) {
    console.log(`[file:${this.path}] ${msg}`)
  }
}

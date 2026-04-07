# Sample Project

> Scaffolded via `scaffold-e2e-tests.sh`.

This project is a sample codebase used for testing the unused code detectors in Kuizer. It contains a mix of live and dead code, including:

- **Live code**: Code that is reachable and used by the entry point (`src/index.ts`).
- **Dead code**: Code that is unreachable or unused, such as utility functions that are never imported, or entire files that have no consumers.

Dead code:
- `src/utils/format.ts`: Exported utility functions that are never imported anywhere.
- `src/utils/math.ts`: Contains utility functions that are never imported anywhere.
- `src/components/Button.ts`: A UI component that is defined but never used.
- `src/components/Modal.ts`: Another UI component that is defined but never used.

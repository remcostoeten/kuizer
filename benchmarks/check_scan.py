import json
import math
import sys
from pathlib import Path


def main() -> int:
    result_path = Path(sys.argv[1])
    result = json.loads(result_path.read_text())
    baseline = json.loads(Path(__file__).with_name("scan-baseline.json").read_text())
    measured = result["results"][0]["median"]
    expected = baseline["median_seconds"]
    threshold = expected * (1 + baseline["max_regression_percent"] / 100)

    if not math.isfinite(measured) or measured <= 0:
        print(f"invalid scan median: {measured}", file=sys.stderr)
        return 1

    print(f"scan median: {measured:.3f}s, baseline: {expected:.3f}s, limit: {threshold:.3f}s")

    if measured > threshold:
        print(
            f"scan median regressed by {(measured / expected - 1) * 100:.1f}% "
            f"(limit {baseline['max_regression_percent']}%)",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())

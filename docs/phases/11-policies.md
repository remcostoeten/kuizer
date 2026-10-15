# Phase 11: policies

Status: plan. A policy is a named set of filters and a selection in the config, specified in `design.md` sections 5 and 6.5. kuizer does not schedule anything itself; a policy is what a cron job or a timer you set up calls.

## 1. Config

`Config` in `src/config/mod.rs` gains `pub policy: BTreeMap<String, Policy>`:

```rust
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub source: SourceKind,          // required: projects, cache, toolchain, docker
    pub paths: Vec<PathBuf>,         // projects only, default scan.roots
    pub category: Vec<Category>,
    pub rule: Vec<String>,
    pub idle: Option<Idle>,
    pub min_size: Option<ByteSize>,
    pub exclude: Vec<String>,
    pub select: PolicySelect,        // "safe", "all" or a list of rule names
    pub permanent: bool,
}
```

`Config::validate` checks every policy: rule names in `rule` and `select` exist for that source (`config.rule_specs()` for projects, `caches::names()` for cache), `paths` is empty unless the source is projects, globs compile. Errors name the policy: `policy.stale-deps.rule: unknown rule "node-modlues"`. `deny_unknown_fields` gives the unknown key error for free.

`docs/config.example.toml` and `config init` output gain one commented example policy.

## 2. Commands

| Command | Behaviour |
| --- | --- |
| `kuizer policy list` | Name, source, selection, and a one-line filter summary per policy. |
| `kuizer policy show NAME` | The effective filters and selection after defaults, the same lines `clean --policy NAME --dry-run` would use. |
| `kuizer clean --policy NAME` | Runs the policy whatever its source: a `cache` policy runs the cache source. Paths on the command line are a usage error for non-project policies. |

`Command::Policy` goes in the "Setup" group of `COMMAND_GROUPS`.

## 3. Flags next to `--policy` narrow, never widen

The policy becomes a `Filter` and a selection (phase 8). CLI flags combine with it:

| Flag | Combined as |
| --- | --- |
| `--category`, `--rule`, `--project` | intersection with the policy |
| `--min-size`, `--idle` | the stricter value wins |
| `--exclude` | union |
| `--select` | allowed only if every value is covered by the policy's selection; `--select all` on a `safe` policy, or a review rule on a `safe` policy, is a usage error |
| `--permanent` | usage error unless the policy has `permanent = true` |
| `--dry-run`, `--yes`, `-q`, `--json` | allowed |

Usage errors exit with code `2`, as `design.md` section 6 says. Today `main.rs` maps every `AppError` to `1`; add `AppError::Usage(String)` and map it to `2`, and move the existing usage-type errors (`UnknownSelection`) onto it.

## 4. History

`Run` gains `policy: Option<String>`. `kuizer history` shows it in the command column, and the dashboard's History tab groups runs by policy, with each policy's last run and the average gap between runs. That is how recurring cleanups show up without kuizer scheduling them.

## 5. Running a policy on a schedule

Documented in the README, not built: a crontab line or a systemd user timer that runs

```sh
kuizer clean --policy stale-deps --yes -q
```

`--yes` is required because there is no terminal to confirm on; without it kuizer refuses, as `removal::run` already does when stdin is not a terminal.

## Tests

- `config check` rejects an unknown policy key, an unknown rule name, and `paths` on a cache policy.
- A policy with `select = "safe"`: `--select all` and `--permanent` are refused with exit code `2`.
- `--min-size` larger than the policy's narrows the result; smaller is ignored.
- `clean --policy NAME --dry-run` lists exactly what `policy show NAME` describes, on a fixed tree.
- A cache policy run through `clean --policy` touches only the caches its selection names.

## Accepted when

The tests above pass and a flag next to `--policy` cannot widen it.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use kuizer::detect::{
    CACHEDIR_SIGNATURE, CACHEDIR_TAG, Category, Detector, Kind, Listing, Match, Safety,
    builtin_rules,
};
use tempfile::TempDir;

#[derive(Clone, Copy)]
enum Entry {
    File(&'static str),
    Dir(&'static str),
    ValidCacheTag,
    InvalidCacheTag,
}

struct Fixture {
    target: &'static str,
    target_kind: Kind,
    siblings: &'static [Entry],
    contents: &'static [Entry],
}

const fn dir(
    target: &'static str,
    siblings: &'static [Entry],
    contents: &'static [Entry],
) -> Fixture {
    Fixture {
        target,
        target_kind: Kind::Dir,
        siblings,
        contents,
    }
}

const fn file(target: &'static str, siblings: &'static [Entry]) -> Fixture {
    Fixture {
        target,
        target_kind: Kind::File,
        siblings,
        contents: &[],
    }
}

struct RuleCase {
    rule: &'static str,
    category: Category,
    safety: Safety,
    ecosystem: &'static str,
    positive: Fixture,
    negative: Fixture,
}

const CASES: &[RuleCase] = &[
    RuleCase {
        rule: "node-modules",
        category: Category::Dependencies,
        safety: Safety::Safe,
        ecosystem: "node",
        positive: dir(
            "node_modules",
            &[Entry::File("package.json")],
            &[Entry::Dir("pkg")],
        ),
        negative: dir(
            "node_modules",
            &[Entry::File("README.md")],
            &[Entry::Dir("pkg")],
        ),
    },
    RuleCase {
        rule: "next-build",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "nextjs",
        positive: dir(
            ".next",
            &[Entry::File("package.json")],
            &[Entry::Dir("cache")],
        ),
        negative: dir(".next", &[Entry::File("README.md")], &[Entry::Dir("cache")]),
    },
    RuleCase {
        rule: "nuxt-build",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "nuxt",
        positive: dir(
            ".nuxt",
            &[Entry::File("package.json")],
            &[Entry::File("tsconfig.json")],
        ),
        negative: dir(
            ".nuxt",
            &[Entry::File("README.md")],
            &[Entry::File("tsconfig.json")],
        ),
    },
    RuleCase {
        rule: "nuxt-output",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "nuxt",
        positive: dir(
            ".output",
            &[Entry::File("nuxt.config.ts")],
            &[Entry::Dir("server")],
        ),
        negative: dir(
            ".output",
            &[Entry::File("package.json")],
            &[Entry::Dir("server")],
        ),
    },
    RuleCase {
        rule: "svelte-kit",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "sveltekit",
        positive: dir(
            ".svelte-kit",
            &[Entry::File("package.json")],
            &[Entry::Dir("output")],
        ),
        negative: dir(
            ".svelte-kit",
            &[Entry::File("README.md")],
            &[Entry::Dir("output")],
        ),
    },
    RuleCase {
        rule: "turbo-cache",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "turborepo",
        positive: dir(
            ".turbo",
            &[Entry::File("turbo.json")],
            &[Entry::Dir("cache")],
        ),
        negative: dir(
            ".turbo",
            &[Entry::File("README.md")],
            &[Entry::Dir("cache")],
        ),
    },
    RuleCase {
        rule: "parcel-cache",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "parcel",
        positive: dir(
            ".parcel-cache",
            &[Entry::File("package.json")],
            &[Entry::File("data.mdb")],
        ),
        negative: dir(
            ".parcel-cache",
            &[Entry::File("README.md")],
            &[Entry::File("data.mdb")],
        ),
    },
    RuleCase {
        rule: "vite-cache",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "vite",
        positive: dir(
            ".vite",
            &[Entry::File("package.json")],
            &[Entry::Dir("deps")],
        ),
        negative: dir(".vite", &[Entry::File("README.md")], &[Entry::Dir("deps")]),
    },
    RuleCase {
        rule: "angular-cache",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "angular",
        positive: dir(
            ".angular",
            &[Entry::File("angular.json")],
            &[Entry::Dir("cache")],
        ),
        negative: dir(
            ".angular",
            &[Entry::File("package.json")],
            &[Entry::Dir("cache")],
        ),
    },
    RuleCase {
        rule: "js-coverage",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "node",
        positive: dir(
            "coverage",
            &[Entry::File("package.json")],
            &[Entry::File("lcov.info")],
        ),
        negative: dir(
            "coverage",
            &[Entry::File("package.json")],
            &[Entry::File("notes.md")],
        ),
    },
    RuleCase {
        rule: "cargo-target",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "rust",
        positive: dir(
            "target",
            &[Entry::File("Cargo.toml")],
            &[Entry::ValidCacheTag],
        ),
        negative: dir(
            "target",
            &[Entry::File("Cargo.toml")],
            &[Entry::InvalidCacheTag],
        ),
    },
    RuleCase {
        rule: "gradle-cache",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "gradle",
        positive: dir(
            ".gradle",
            &[Entry::File("build.gradle.kts")],
            &[Entry::Dir("8.10")],
        ),
        negative: dir(".gradle", &[Entry::File("pom.xml")], &[Entry::Dir("8.10")]),
    },
    RuleCase {
        rule: "python-bytecode",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "python",
        positive: dir("__pycache__", &[], &[Entry::File("app.cpython-312.pyc")]),
        negative: file("__pycache__", &[]),
    },
    RuleCase {
        rule: "python-venv",
        category: Category::Dependencies,
        safety: Safety::Review,
        ecosystem: "python",
        positive: dir(
            ".venv",
            &[Entry::File("pyproject.toml")],
            &[Entry::File("pyvenv.cfg")],
        ),
        negative: dir(
            ".venv",
            &[Entry::File("pyproject.toml")],
            &[Entry::Dir("lib")],
        ),
    },
    RuleCase {
        rule: "tox-env",
        category: Category::Dependencies,
        safety: Safety::Safe,
        ecosystem: "python",
        positive: dir(".tox", &[Entry::File("tox.ini")], &[Entry::Dir("py312")]),
        negative: dir(".tox", &[Entry::File("README.md")], &[Entry::Dir("py312")]),
    },
    RuleCase {
        rule: "nox-env",
        category: Category::Dependencies,
        safety: Safety::Safe,
        ecosystem: "python",
        positive: dir(".nox", &[Entry::File("noxfile.py")], &[Entry::Dir("tests")]),
        negative: dir(
            ".nox",
            &[Entry::File("pyproject.toml")],
            &[Entry::Dir("tests")],
        ),
    },
    RuleCase {
        rule: "pytest-cache",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "python",
        positive: dir(".pytest_cache", &[], &[Entry::ValidCacheTag]),
        negative: dir(".pytest_cache", &[], &[Entry::File("README.md")]),
    },
    RuleCase {
        rule: "mypy-cache",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "python",
        positive: dir(".mypy_cache", &[], &[Entry::ValidCacheTag]),
        negative: dir(".mypy_cache", &[], &[Entry::InvalidCacheTag]),
    },
    RuleCase {
        rule: "ruff-cache",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "python",
        positive: dir(".ruff_cache", &[], &[Entry::ValidCacheTag]),
        negative: dir(".ruff_cache", &[], &[Entry::File("0.8.0")]),
    },
    RuleCase {
        rule: "dotnet-bin",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "dotnet",
        positive: dir("bin", &[Entry::File("App.csproj")], &[Entry::Dir("Debug")]),
        negative: dir(
            "bin",
            &[Entry::File("App.csproj")],
            &[Entry::File("deploy.sh")],
        ),
    },
    RuleCase {
        rule: "dotnet-obj",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "dotnet",
        positive: dir(
            "obj",
            &[Entry::File("Lib.fsproj")],
            &[Entry::File("project.assets.json")],
        ),
        negative: dir(
            "obj",
            &[Entry::File("README.md")],
            &[Entry::File("project.assets.json")],
        ),
    },
    RuleCase {
        rule: "cocoapods",
        category: Category::Dependencies,
        safety: Safety::Review,
        ecosystem: "cocoapods",
        positive: dir(
            "Pods",
            &[Entry::File("Podfile")],
            &[Entry::File("Manifest.lock")],
        ),
        negative: dir(
            "Pods",
            &[Entry::File("Podfile")],
            &[Entry::File("notes.md")],
        ),
    },
    RuleCase {
        rule: "elixir-build",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "elixir",
        positive: dir("_build", &[Entry::File("mix.exs")], &[Entry::Dir("dev")]),
        negative: dir("_build", &[Entry::File("Makefile")], &[Entry::Dir("dev")]),
    },
    RuleCase {
        rule: "elixir-deps",
        category: Category::Dependencies,
        safety: Safety::Safe,
        ecosystem: "elixir",
        positive: dir("deps", &[Entry::File("mix.exs")], &[Entry::Dir("phoenix")]),
        negative: dir("deps", &[Entry::File("Makefile")], &[Entry::Dir("phoenix")]),
    },
    RuleCase {
        rule: "zig-cache",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "zig",
        positive: dir(
            ".zig-cache",
            &[Entry::File("build.zig")],
            &[Entry::Dir("o")],
        ),
        negative: dir(".zig-cache", &[Entry::File("main.zig")], &[Entry::Dir("o")]),
    },
    RuleCase {
        rule: "zig-out",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "zig",
        positive: dir("zig-out", &[Entry::File("build.zig")], &[Entry::Dir("bin")]),
        negative: dir("zig-out", &[Entry::File("README.md")], &[Entry::Dir("bin")]),
    },
    RuleCase {
        rule: "haskell-stack",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "haskell",
        positive: dir(
            ".stack-work",
            &[Entry::File("stack.yaml")],
            &[Entry::Dir("dist")],
        ),
        negative: dir(
            ".stack-work",
            &[Entry::File("README.md")],
            &[Entry::Dir("dist")],
        ),
    },
    RuleCase {
        rule: "haskell-cabal",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "haskell",
        positive: dir(
            "dist-newstyle",
            &[Entry::File("app.cabal")],
            &[Entry::Dir("cache")],
        ),
        negative: dir(
            "dist-newstyle",
            &[Entry::File("Setup.hs")],
            &[Entry::Dir("cache")],
        ),
    },
    RuleCase {
        rule: "swift-build",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "swift",
        positive: dir(
            ".build",
            &[Entry::File("Package.swift")],
            &[Entry::Dir("debug")],
        ),
        negative: dir(".build", &[Entry::File("Makefile")], &[Entry::Dir("debug")]),
    },
    RuleCase {
        rule: "maven-target",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "maven",
        positive: dir(
            "target",
            &[Entry::File("pom.xml")],
            &[Entry::Dir("classes")],
        ),
        negative: dir(
            "target",
            &[Entry::File("pom.xml")],
            &[Entry::File("deploy.yaml")],
        ),
    },
    RuleCase {
        rule: "terraform",
        category: Category::Cache,
        safety: Safety::Review,
        ecosystem: "terraform",
        positive: dir(
            ".terraform",
            &[Entry::File("main.tf")],
            &[Entry::Dir("providers")],
        ),
        negative: dir(
            ".terraform",
            &[Entry::File("main.tf.bak")],
            &[Entry::Dir("providers")],
        ),
    },
    RuleCase {
        rule: "gradle-build",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "gradle",
        positive: dir(
            "build",
            &[Entry::File("build.gradle")],
            &[Entry::Dir("classes")],
        ),
        negative: dir(
            "build",
            &[Entry::File("build.gradle")],
            &[Entry::File("index.html")],
        ),
    },
    RuleCase {
        rule: "dart-tool",
        category: Category::Cache,
        safety: Safety::Safe,
        ecosystem: "dart",
        positive: dir(
            ".dart_tool",
            &[Entry::File("pubspec.yaml")],
            &[Entry::File("package_config.json")],
        ),
        negative: dir(
            ".dart_tool",
            &[Entry::File("README.md")],
            &[Entry::File("package_config.json")],
        ),
    },
    RuleCase {
        rule: "flutter-build",
        category: Category::Build,
        safety: Safety::Review,
        ecosystem: "flutter",
        positive: dir(
            "build",
            &[Entry::File("pubspec.yaml")],
            &[Entry::Dir("web")],
        ),
        negative: dir(
            "build",
            &[Entry::File("analysis_options.yaml")],
            &[Entry::Dir("web")],
        ),
    },
    RuleCase {
        rule: "cmake-build",
        category: Category::Build,
        safety: Safety::Review,
        ecosystem: "cmake",
        positive: dir(
            "cmake-build-debug",
            &[Entry::File("CMakeLists.txt")],
            &[Entry::File("CMakeCache.txt")],
        ),
        negative: dir(
            "cmake-build-debug",
            &[Entry::File("CMakeLists.txt")],
            &[Entry::File("notes.md")],
        ),
    },
    RuleCase {
        rule: "unity-library",
        category: Category::Cache,
        safety: Safety::Review,
        ecosystem: "unity",
        positive: dir(
            "Library",
            &[Entry::Dir("ProjectSettings")],
            &[Entry::File("ArtifactDB")],
        ),
        negative: dir(
            "Library",
            &[Entry::Dir("ProjectSettings")],
            &[Entry::Dir("Fonts")],
        ),
    },
    RuleCase {
        rule: "unity-temp",
        category: Category::Temp,
        safety: Safety::Review,
        ecosystem: "unity",
        positive: dir(
            "Temp",
            &[Entry::Dir("ProjectSettings")],
            &[Entry::File("UnityTempFile-1")],
        ),
        negative: dir(
            "Temp",
            &[Entry::Dir("Assets")],
            &[Entry::File("UnityTempFile-1")],
        ),
    },
    RuleCase {
        rule: "typescript-buildinfo",
        category: Category::Build,
        safety: Safety::Safe,
        ecosystem: "typescript",
        positive: file("tsconfig.tsbuildinfo", &[Entry::File("tsconfig.json")]),
        negative: dir("tsconfig.tsbuildinfo", &[Entry::File("tsconfig.json")], &[]),
    },
    RuleCase {
        rule: "js-tool-cache",
        category: Category::Cache,
        safety: Safety::Review,
        ecosystem: "node",
        positive: dir(
            ".cache",
            &[Entry::File("package.json")],
            &[Entry::Dir("babel")],
        ),
        negative: dir(
            ".cache",
            &[Entry::File("README.md")],
            &[Entry::Dir("fontconfig")],
        ),
    },
    RuleCase {
        rule: "js-dist",
        category: Category::Build,
        safety: Safety::Review,
        ecosystem: "node",
        positive: dir(
            "dist",
            &[Entry::File("package.json")],
            &[Entry::File("index.js")],
        ),
        negative: dir(
            "dist",
            &[Entry::File("README.md")],
            &[Entry::File("index.js")],
        ),
    },
    RuleCase {
        rule: "js-build",
        category: Category::Build,
        safety: Safety::Review,
        ecosystem: "node",
        positive: dir(
            "build",
            &[Entry::File("package.json")],
            &[Entry::File("bundle.js")],
        ),
        negative: dir(
            "build",
            &[Entry::File("README.md")],
            &[Entry::File("bundle.js")],
        ),
    },
    RuleCase {
        rule: "js-out",
        category: Category::Build,
        safety: Safety::Review,
        ecosystem: "node",
        positive: dir(
            "out",
            &[Entry::File("package.json")],
            &[Entry::File("index.html")],
        ),
        negative: dir(
            "out",
            &[Entry::File("README.md")],
            &[Entry::File("index.html")],
        ),
    },
    RuleCase {
        rule: "cachedir-tag",
        category: Category::Cache,
        safety: Safety::Review,
        ecosystem: "generic",
        positive: dir("custom-cache", &[], &[Entry::ValidCacheTag]),
        negative: dir("custom-cache", &[], &[Entry::InvalidCacheTag]),
    },
];

fn materialize_entry(dir: &Path, entry: Entry) {
    match entry {
        Entry::File(name) => {
            fs::write(dir.join(name), b"x").expect("write fixture file");
        }
        Entry::Dir(name) => {
            fs::create_dir(dir.join(name)).expect("create fixture dir");
        }
        Entry::ValidCacheTag => {
            fs::write(dir.join(CACHEDIR_TAG), CACHEDIR_SIGNATURE)
                .expect("write valid cachedir tag");
        }
        Entry::InvalidCacheTag => {
            fs::write(
                dir.join(CACHEDIR_TAG),
                b"Signature: 00000000000000000000000000000000",
            )
            .expect("write invalid cachedir tag");
        }
    }
}

fn listing_of(detector: &Detector, dir: &Path) -> Listing {
    let mut listing = Listing::default();
    for entry in fs::read_dir(dir).expect("read fixture dir") {
        let entry = entry.expect("fixture dir entry");
        detector.note(&mut listing, &entry.file_name());
    }
    listing
}

fn classify(detector: &Detector, project_dir: &Path, fixture: &Fixture) -> Option<Match> {
    fs::create_dir_all(project_dir).expect("create project dir");
    for &entry in fixture.siblings {
        materialize_entry(project_dir, entry);
    }

    let target_path = project_dir.join(fixture.target);
    match fixture.target_kind {
        Kind::Dir => {
            fs::create_dir(&target_path).expect("create target dir");
            for &entry in fixture.contents {
                materialize_entry(&target_path, entry);
            }
            let parent = listing_of(detector, project_dir);
            let own = listing_of(detector, &target_path);
            detector.classify_dir(&target_path, &parent, &own)
        }
        Kind::File => {
            assert!(
                fixture.contents.is_empty(),
                "file fixture `{}` cannot have directory contents",
                fixture.target
            );
            fs::write(&target_path, b"x").expect("write target file");
            let parent = listing_of(detector, project_dir);
            detector.classify_file(target_path.file_name().expect("target file name"), &parent)
        }
        Kind::External => unreachable!("built-in rules only use dir or file kinds"),
    }
}

#[test]
fn every_builtin_rule_has_positive_and_negative_case() {
    let rules = builtin_rules();
    let detector = Detector::new(&rules).expect("builtin rules compile");

    let builtin_names: BTreeSet<&str> = rules.iter().map(|rule| rule.name.as_str()).collect();
    let mut tested_names = BTreeSet::new();
    for case in CASES {
        assert!(
            tested_names.insert(case.rule),
            "duplicate test case for built-in rule `{}`",
            case.rule
        );
    }
    let missing: Vec<&str> = builtin_names.difference(&tested_names).copied().collect();
    let extra: Vec<&str> = tested_names.difference(&builtin_names).copied().collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "builtin_rules() and CASES diverged: missing={missing:?}, extra={extra:?}"
    );

    let tmp = TempDir::new().expect("tempdir");
    for case in CASES {
        let case_root = tmp.path().join(case.rule);

        let positive_hit = classify(&detector, &case_root.join("positive"), &case.positive)
            .unwrap_or_else(|| {
                panic!(
                    "expected built-in rule `{}` to match positive fixture `{}`",
                    case.rule, case.positive.target
                )
            });
        assert_eq!(
            positive_hit.name, case.rule,
            "positive fixture for `{}` matched `{}` instead",
            case.rule, positive_hit.name
        );
        assert_eq!(
            positive_hit.category, case.category,
            "unexpected category for `{}`",
            case.rule
        );
        assert_eq!(
            positive_hit.safety, case.safety,
            "unexpected safety for `{}`",
            case.rule
        );
        assert_eq!(
            positive_hit.ecosystem, case.ecosystem,
            "unexpected ecosystem for `{}`",
            case.rule
        );

        let negative_hit = classify(&detector, &case_root.join("negative"), &case.negative);
        assert!(
            negative_hit.is_none(),
            "expected negative fixture `{}` for `{}` not to match, but matched {:?}",
            case.negative.target,
            case.rule,
            negative_hit.as_ref().map(|m| m.name.as_str())
        );
    }
}

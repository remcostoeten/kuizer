use std::fs;
use std::path::{Path, PathBuf};

fn write(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().expect("has parent")).expect("create parent");
    fs::write(path, vec![b'x'; bytes]).expect("write file");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().expect("usage: synth <dir> [projects] [deps]"));
    let projects: usize = args.next().map_or(200, |v| v.parse().expect("projects"));
    let deps: usize = args.next().map_or(100, |v| v.parse().expect("deps"));
    for project in 0..projects {
        let base = root.join(format!("project-{project}"));
        write(&base.join("package.json"), 200);
        for module in 0..20 {
            write(&base.join(format!("src/feature-{module}/index.ts")), 2_000);
        }
        for dep in 0..deps {
            let dir = base.join(format!("node_modules/dep-{dep}"));
            write(&dir.join("package.json"), 500);
            write(&dir.join("index.js"), 8_000);
            write(&dir.join("lib/util.js"), 4_000);
        }
        if project % 4 == 0 {
            write(&base.join("dist/app.js"), 50_000);
        }
    }
}

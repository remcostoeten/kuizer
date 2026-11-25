# What each source removes

Kuizer reports items from each source. Scanning only reads files and metadata. Removal runs through the source-specific command or the guarded deletion module after you select items.

## Projects

The default project scan detects dependency folders, build output, project caches and temporary files when project markers support the match. `kuizer clean` removes selected project items. In trash mode, these items move to the system trash.

## Global caches

`kuizer cache` lists configured tool caches, including npm, pnpm, Cargo, pip and uv caches. `kuizer cache clean` removes selected cache paths. Trash mode applies.

## Toolchains

`kuizer toolchain` lists installed Rust, Node.js, Python, SDKMAN and Go versions in supported locations. `kuizer toolchain clean` invokes the tool's uninstaller when available, or removes the version directory. Toolchains are always removed permanently.

## Docker and Podman

`kuizer docker` lists dangling images, build cache, stopped containers and orphaned volumes reported by each configured engine. `kuizer docker clean` calls the engine CLI to remove selected objects. Kuizer does not delete engine storage directories. These objects are always removed permanently.

## Git

`kuizer git` reports repositories with reclaimable loose objects and eligible merged local branches. `kuizer git --gc` runs `git gc`. `kuizer git --branches` deletes selected merged branches. Branch removal is always permanent. Kuizer prints the branch and commit command needed to restore a deleted branch reference.

## Duplicates

`kuizer duplicates` reports files with identical contents. It does not remove or select copies because it cannot determine which copy you want to keep.

## Trash and history

Set `delete.mode = "trash"` for project artifacts and global caches. Toolchains, Docker and Podman objects, and Git branches remain permanent. `kuizer history` records removal results, and `kuizer restore` restores items that remain in the trash when the platform supports restoration.

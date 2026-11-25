use crate::config::DeleteMode;
use crate::delete::Mode;

/// Where a removable item comes from. Each source has its own lister and remover, and the core
/// decides here which of them may go to the trash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Artifact,
    Cache,
    Toolchain,
    Docker,
    Trash,
}

impl Source {
    pub const ALL: [Source; 5] = [
        Self::Artifact,
        Self::Cache,
        Self::Toolchain,
        Self::Docker,
        Self::Trash,
    ];

    /// @name Source::permanent
    /// @description Whether items of this source are always deleted for good, whatever
    /// `delete.mode` says. Toolchains are removed through their own uninstaller, Docker
    /// objects through the engine, and the trash itself cannot go to the trash.
    ///
    /// @example
    /// assert!(Source::Docker.permanent());
    pub fn permanent(self) -> bool {
        matches!(self, Self::Toolchain | Self::Docker | Self::Trash)
    }

    /// @name Source::trashed
    /// @description Whether a removal of this source under `mode` ends up in the trash.
    ///
    /// @example
    /// let label = if Source::Cache.trashed(config.delete.mode) { "Move to trash" } else { "Delete" };
    pub fn trashed(self, mode: DeleteMode) -> bool {
        mode == DeleteMode::Trash && !self.permanent()
    }

    /// @name Source::mode
    /// @description The removal mode this source actually runs with when `wanted` is asked for:
    /// a dry run stays a dry run, and a trash request becomes permanent for a permanent source.
    ///
    /// @example
    /// let mode = Source::Toolchain.mode(removal::mode(&args, &config));
    pub fn mode(self, wanted: Mode) -> Mode {
        match wanted {
            Mode::Trash if self.permanent() => Mode::Permanent,
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_artifacts_and_caches_reach_the_trash() {
        let trashable: Vec<Source> = Source::ALL
            .into_iter()
            .filter(|source| source.trashed(DeleteMode::Trash))
            .collect();
        assert_eq!(trashable, [Source::Artifact, Source::Cache]);
        assert!(
            Source::ALL
                .iter()
                .all(|source| !source.trashed(DeleteMode::Permanent))
        );
        assert_eq!(Source::Docker.mode(Mode::Trash), Mode::Permanent);
        assert_eq!(Source::Toolchain.mode(Mode::DryRun), Mode::DryRun);
        assert_eq!(Source::Cache.mode(Mode::Trash), Mode::Trash);
    }
}

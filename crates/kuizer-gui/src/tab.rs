use kuizer::dupes;

use crate::glossary::Term;
use crate::model::Stage;
use crate::start::ScanOptions;

/// One section of the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Overview,
    Artifacts,
    Caches,
    Duplicates,
    History,
    Issues,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Self::Overview,
        Self::Artifacts,
        Self::Caches,
        Self::Duplicates,
        Self::History,
        Self::Issues,
    ];

    /// The next section in the sidebar, wrapping around.
    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|&tab| tab == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    /// The previous section in the sidebar, wrapping around.
    pub fn previous(self) -> Self {
        let index = Self::ALL.iter().position(|&tab| tab == self).unwrap_or(0);
        Self::ALL[(index + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn term(self) -> Term {
        match self {
            Self::Overview => Term::Overview,
            Self::Artifacts => Term::ProjectArtifacts,
            Self::Caches => Term::GlobalCaches,
            Self::Duplicates => Term::DuplicateFiles,
            Self::History => Term::History,
            Self::Issues => Term::Issues,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Artifacts => "Project artifacts",
            Self::Caches => "Global caches",
            Self::Duplicates => "Duplicates",
            Self::History => "History",
            Self::Issues => "Issues",
        }
    }

    pub fn status(self, stage: Stage, options: &ScanOptions) -> &'static str {
        let walking = matches!(stage, Stage::Starting | Stage::Scanning(_));
        match self {
            Self::Overview => "Shown once every part of the scan is done",
            Self::Artifacts if walking => "Looking for project artifacts",
            Self::Artifacts => "Artifacts found, shown once the scan is done",
            Self::Issues if walking => "Collecting issues while scanning folders",
            Self::Issues => "Issues collected, shown once the scan is done",
            Self::Duplicates if !options.duplicates => "Duplicate search is off for this scan",
            Self::Duplicates if walking => "Collecting files while scanning folders",
            Self::Duplicates => match stage {
                Stage::Duplicates(dupes::Stage::Walking { .. }) => "Collecting files",
                Stage::Duplicates(dupes::Stage::Hashing { .. }) => "Hashing files",
                Stage::Duplicates(dupes::Stage::Verifying { .. }) => "Comparing files",
                _ => "Duplicates found, shown once the scan is done",
            },
            Self::Caches if !options.caches => "Global caches are off for this scan",
            Self::Caches => match stage {
                Stage::Caches => "Measuring global caches",
                Stage::History => "Caches measured, shown once the scan is done",
                _ => "Measured after the folder scan",
            },
            Self::History if matches!(stage, Stage::History) => "Reading the cleanup history",
            Self::History => "Read at the end of the scan",
        }
    }
}

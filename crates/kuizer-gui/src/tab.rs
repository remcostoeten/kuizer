use crate::activity::{Activity, State, Step};
use crate::glossary::Term;
use crate::icon::Icon;
use crate::model::Part;
use crate::start::ScanOptions;

/// One section of the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Overview,
    Artifacts,
    Caches,
    Toolchains,
    Docker,
    Repos,
    Duplicates,
    History,
    Issues,
}

impl Tab {
    pub const ALL: [Tab; 9] = [
        Self::Overview,
        Self::Artifacts,
        Self::Caches,
        Self::Toolchains,
        Self::Docker,
        Self::Repos,
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

    /// Whether rows in this section can be selected for removal.
    pub fn selects(self) -> bool {
        matches!(
            self,
            Self::Artifacts | Self::Caches | Self::Toolchains | Self::Docker
        )
    }

    pub fn term(self) -> Term {
        match self {
            Self::Overview => Term::Overview,
            Self::Artifacts => Term::ProjectArtifacts,
            Self::Caches => Term::GlobalCaches,
            Self::Toolchains => Term::Toolchains,
            Self::Docker => Term::Docker,
            Self::Repos => Term::GitRepositories,
            Self::Duplicates => Term::DuplicateFiles,
            Self::History => Term::History,
            Self::Issues => Term::Issues,
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Self::Overview => Icon::Overview,
            Self::Artifacts => Icon::Artifacts,
            Self::Caches => Icon::Caches,
            Self::Toolchains => Icon::Toolchains,
            Self::Docker => Icon::Docker,
            Self::Repos => Icon::Repos,
            Self::Duplicates => Icon::Duplicates,
            Self::History => Icon::History,
            Self::Issues => Icon::Issues,
        }
    }

    /// The group heading the sidebar shows above this section, on the first section of a group.
    pub fn heading(self) -> Option<&'static str> {
        match self {
            Self::Artifacts => Some("Reclaim"),
            Self::Repos => Some("Tidy up"),
            Self::History => Some("Activity"),
            _ => None,
        }
    }

    /// The keys that open this section: its place in the sidebar.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Overview => &["1"],
            Self::Artifacts => &["2"],
            Self::Caches => &["3"],
            Self::Toolchains => &["4"],
            Self::Docker => &["5"],
            Self::Repos => &["6"],
            Self::Duplicates => &["7"],
            Self::History => &["8"],
            Self::Issues => &["9"],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Artifacts => "Project artifacts",
            Self::Caches => "Global caches",
            Self::Toolchains => "Toolchains",
            Self::Docker => "Docker",
            Self::Repos => "Git repositories",
            Self::Duplicates => "Duplicates",
            Self::History => "History",
            Self::Issues => "Issues",
        }
    }

    /// The part of a scan that fills this section, for the ones a scan delivers.
    pub fn part(self) -> Option<Part> {
        match self {
            Self::Overview | Self::Duplicates => None,
            Self::Artifacts | Self::Issues => Some(Part::Artifacts),
            Self::Caches => Some(Part::Caches),
            Self::Toolchains => Some(Part::Toolchains),
            Self::Docker => Some(Part::Docker),
            Self::Repos => Some(Part::Repos),
            Self::History => Some(Part::History),
        }
    }

    pub fn status(self, activity: &Activity, options: &ScanOptions) -> &'static str {
        let step = match self {
            Self::Overview => return "Shown once every part of the scan is done",
            Self::Duplicates if options.duplicates => {
                return "Searched once the rest of the scan is on screen";
            }
            Self::Duplicates => return "Searched on request once the scan is done",
            Self::Artifacts | Self::Issues => Step::Folders,
            Self::Caches => Step::Caches,
            Self::Toolchains => Step::Toolchains,
            Self::Docker => Step::Docker,
            Self::Repos => Step::Repos,
            Self::History => Step::History,
        };
        let state = activity.state(step, options);
        match self {
            _ if matches!(state, State::Done(_)) => "Done, shown with the first project artifacts",
            _ if state == State::Off => match self {
                Self::Caches => "Global caches are off for this scan",
                Self::Toolchains => "Toolchains are off for this scan",
                Self::Docker => "Docker and Podman are off for this scan",
                _ => "Git repositories are off for this scan",
            },
            Self::Artifacts => "Looking for project artifacts",
            Self::Issues => "Collecting issues while scanning folders",
            Self::Caches => "Measuring global caches",
            Self::Toolchains => "Measuring installed toolchains",
            Self::Docker => "Asking Docker and Podman",
            Self::Repos if state == State::Pending => {
                "Noting git repositories while scanning folders"
            }
            Self::Repos => "Checking git repositories",
            _ => "Reading the cleanup history",
        }
    }
}

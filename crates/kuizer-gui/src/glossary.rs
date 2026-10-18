use kuizer::detect::{Category, Safety};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Term {
    Overview,
    History,
    Trash,
    Safety,
    Category,
    Safe,
    Review,
    Dependencies,
    Build,
    Cache,
    Temp,
    Rule,
    Changed,
    Size,
    Tool,
    ProjectArtifacts,
    GlobalCaches,
    DuplicateFiles,
    Reclaimable,
    Groups,
    FilesCompared,
    Unverified,
    DiskFree,
    SelectSafe,
    Cleanups,
    ItemsRemoved,
    Freed,
    InTrash,
    Restore,
    Run,
    Mode,
    Command,
    Issues,
    GitRepositories,
    GcGain,
    LooseObjects,
    MergedBranches,
}

impl Term {
    #[cfg(test)]
    pub const ALL: [Term; 37] = [
        Self::Overview,
        Self::History,
        Self::Trash,
        Self::Safety,
        Self::Category,
        Self::Safe,
        Self::Review,
        Self::Dependencies,
        Self::Build,
        Self::Cache,
        Self::Temp,
        Self::Rule,
        Self::Changed,
        Self::Size,
        Self::Tool,
        Self::ProjectArtifacts,
        Self::GlobalCaches,
        Self::DuplicateFiles,
        Self::Reclaimable,
        Self::Groups,
        Self::FilesCompared,
        Self::Unverified,
        Self::DiskFree,
        Self::SelectSafe,
        Self::Cleanups,
        Self::ItemsRemoved,
        Self::Freed,
        Self::InTrash,
        Self::Restore,
        Self::Run,
        Self::Mode,
        Self::Command,
        Self::Issues,
        Self::GitRepositories,
        Self::GcGain,
        Self::LooseObjects,
        Self::MergedBranches,
    ];

    pub fn safety(safety: Safety) -> Self {
        match safety {
            Safety::Safe => Self::Safe,
            Safety::Review => Self::Review,
        }
    }

    pub fn category(category: Category) -> Self {
        match category {
            Category::Dependencies => Self::Dependencies,
            Category::Build => Self::Build,
            Category::Cache => Self::Cache,
            Category::Temp => Self::Temp,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::History => "History",
            Self::Trash => "Move to trash",
            Self::Safety => "Safety",
            Self::Category => "Category",
            Self::Safe => "safe",
            Self::Review => "review",
            Self::Dependencies => "dependencies",
            Self::Build => "build",
            Self::Cache => "cache",
            Self::Temp => "temp",
            Self::Rule => "Rule",
            Self::Changed => "Changed",
            Self::Size => "Size",
            Self::Tool => "Tool",
            Self::ProjectArtifacts => "Project artifacts",
            Self::GlobalCaches => "Global caches",
            Self::DuplicateFiles => "Duplicate files",
            Self::Reclaimable => "Reclaimable",
            Self::Groups => "Groups",
            Self::FilesCompared => "Files compared",
            Self::Unverified => "Not byte-verified",
            Self::DiskFree => "Disk free",
            Self::SelectSafe => "Select safe",
            Self::Cleanups => "Cleanups",
            Self::ItemsRemoved => "Items removed",
            Self::Freed => "Freed",
            Self::InTrash => "Still in the trash",
            Self::Restore => "Trash",
            Self::Run => "Run",
            Self::Mode => "Mode",
            Self::Command => "Command",
            Self::Issues => "Issues",
            Self::GitRepositories => "Git repositories",
            Self::GcGain => "git gc gain",
            Self::LooseObjects => "Loose objects",
            Self::MergedBranches => "Merged branches",
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Self::Overview => {
                "Totals for this scan: what is safe to remove, what needs review, and the largest projects and caches."
            }
            Self::History => {
                "Every cleanup kuizer made on this machine, from this window and from the command line, and what is still in the trash."
            }
            Self::Trash => {
                "Moves the selection to the trash after you confirm. Undo right after, Restore in History or kuizer restore puts it back until the trash is emptied. Nothing is deleted for good."
            }
            Self::Safety => {
                "How sure kuizer is that removing an item is harmless. safe items are rebuilt by a tool, review items need a look first."
            }
            Self::Category => {
                "What kind of generated data this is: dependencies, build output, cache or temp."
            }
            Self::Safe => {
                "A tool rebuilds this from your project files, for example on the next install or build. Removing it only costs the time to rebuild."
            }
            Self::Review => {
                "Probably generated, but it may hold something a rebuild cannot bring back, such as a virtual environment with local changes or a pinned toolchain. Look before you remove it. Review items are never selected in bulk, and moving them asks you to type the item count."
            }
            Self::Dependencies => {
                "Installed packages such as node_modules, Pods and Python virtual environments. Your package manager installs them again."
            }
            Self::Build => {
                "Compiler and bundler output such as target, dist and .next. The next build creates it again."
            }
            Self::Cache => {
                "Data a tool keeps to work faster, such as __pycache__ or .turbo. Tools refill it on their own."
            }
            Self::Temp => {
                "Scratch folders a tool recreates while it runs, such as the Temp folder of a Unity project."
            }
            Self::Rule => {
                "The detection rule that matched this item. A rule also checks the project around it, so a dist folder only counts next to a package.json."
            }
            Self::Changed => {
                "How long ago the newest file inside was modified. Old items are usually from projects you no longer work on."
            }
            Self::Size => {
                "Space the item takes on disk, which is what you get back when the trash is emptied."
            }
            Self::Tool => "The program that owns this cache.",
            Self::ProjectArtifacts => {
                "Generated folders and files inside your projects, such as dependencies and build output."
            }
            Self::GlobalCaches => {
                "Caches that package managers and tools keep in your home folder, shared by all projects. Safe caches fill up again on the next install or build."
            }
            Self::DuplicateFiles => {
                "Files with exactly the same content in more than one place. Keeping one copy of each frees the rest."
            }
            Self::Reclaimable => {
                "Space you would get back by keeping one copy of each duplicate and removing the others."
            }
            Self::Groups => "Each group is a set of files with identical content.",
            Self::FilesCompared => {
                "Files that had the same size as another file and were hashed to compare their content."
            }
            Self::Unverified => {
                "These files have the same hash, but were not compared byte for byte. A hash match is almost always a real duplicate."
            }
            Self::DiskFree => "Free space on the disk that holds the scanned folder.",
            Self::SelectSafe => {
                "Selects every safe item in this tab. Review items stay unselected so you can decide on each one."
            }
            Self::Cleanups => "Every run of kuizer that moved or deleted items.",
            Self::ItemsRemoved => {
                "All items removed over time. Items you put back with kuizer restore are counted as restored."
            }
            Self::Freed => {
                "Space actually given back: items deleted permanently, or moved to the trash that has since been emptied."
            }
            Self::InTrash => {
                "Removed items that are still in the trash. Restore in the table below or kuizer restore puts them back until the trash is emptied."
            }
            Self::Restore => {
                "Whether this cleanup is still in the trash. Restore puts what is left back where it was. Items restored before or emptied from the trash cannot come back."
            }
            Self::Run => {
                "The id of the cleanup. Run kuizer restore with this id, for example kuizer restore s4, to put it back from the trash."
            }
            Self::Mode => {
                "trash moves items to the trash, permanent deletes them for good, and dry run only showed what would happen."
            }
            Self::Command => "The command that started this cleanup.",
            Self::Issues => {
                "Folders the scan could not read or chose to skip, such as other disks or protected folders. Nothing in them is listed."
            }
            Self::GitRepositories => {
                "Git repositories under the scanned folders that have loose objects git gc can pack, or local branches already merged into the default branch."
            }
            Self::GcGain => {
                "An estimate of what git gc frees: the loose objects it packs and the garbage it removes. Packing keeps a compressed copy, so the real gain is a little lower."
            }
            Self::LooseObjects => {
                "Objects git stores as separate files. Every commit adds some until git gc packs them into one compressed file."
            }
            Self::MergedBranches => {
                "Local branches whose last commit is already in the default branch. Deleting one loses nothing: the commits stay in the default branch and remote branches are never touched."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_term_has_a_title_and_an_explanation() {
        for term in Term::ALL {
            assert!(!term.title().is_empty(), "{term:?} has no title");
            assert!(term.text().ends_with('.'), "{term:?} is not a sentence");
        }
    }

    #[test]
    fn safety_and_category_map_to_their_terms() {
        assert_eq!(
            Term::safety(Safety::Review).title(),
            Safety::Review.as_str()
        );
        for category in Category::ALL {
            assert_eq!(Term::category(category).title(), category.as_str());
        }
    }
}

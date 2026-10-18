//! The tree behind the space map: scan roots, then folders, then the reclaimable items found
//! in them, with sizes added up the path. Pure data, built from a report's paths.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use kuizer::detect::{Category, Safety};
use kuizer::output::display_path;

use crate::model::Source;

/// Tiles below this share of their level are grouped into one "smaller" tile.
pub const MIN_SHARE: f64 = 0.004;
/// The most tiles one level shows; the rest are grouped into one "smaller" tile.
pub const MAX_TILES: usize = 80;

const CACHES: &str = "\0caches";
const ELSEWHERE: &str = "\0elsewhere";
const REST: &str = "\0rest";

/// A reclaimable item the map shows as a leaf tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub source: Source,
    pub id: u64,
    pub category: Category,
    pub safety: Safety,
}

/// One item of the report, as the tree builder takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leaf {
    pub path: PathBuf,
    pub bytes: u64,
    pub item: Item,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// The whole map, a scan root, the global caches or items outside every root.
    Group,
    Folder,
    Item(Item),
    /// Several small tiles of one level, grouped. They are kept as its children.
    Rest(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Stable across rebuilds: the path for folders and items, a fixed name for groups.
    pub key: String,
    pub name: String,
    pub path: Option<PathBuf>,
    pub bytes: u64,
    pub items: usize,
    pub kind: Kind,
    /// Largest first, with at most one `Rest` tile at the end.
    pub children: Vec<Node>,
}

impl Node {
    /// Whether clicking or Enter opens this tile as a level of its own.
    pub fn opens(&self) -> bool {
        matches!(self.kind, Kind::Group | Kind::Folder) && !self.children.is_empty()
    }

    pub fn item(&self) -> Option<Item> {
        match self.kind {
            Kind::Item(item) => Some(item),
            _ => None,
        }
    }

    pub fn child(&self, key: &str) -> Option<&Node> {
        self.children.iter().find(|child| child.key == key)
    }

    /// Every item in this subtree with its size, including the grouped small ones.
    pub fn leaves(&self) -> Vec<(Item, u64)> {
        let mut found = Vec::new();
        self.collect(&mut found);
        found
    }

    fn collect(&self, found: &mut Vec<(Item, u64)>) {
        match self.kind {
            Kind::Item(item) => found.push((item, self.bytes)),
            _ => {
                for child in &self.children {
                    child.collect(found);
                }
            }
        }
    }

    /// The nodes from this one down along `path`, stopping at the first key that is gone.
    pub fn trail(&self, path: &[String]) -> Vec<&Node> {
        let mut trail = vec![self];
        for key in path {
            let Some(next) = trail.last().and_then(|node| node.child(key)) else {
                break;
            };
            if !next.opens() {
                break;
            }
            trail.push(next);
        }
        trail
    }
}

#[derive(Default)]
struct Trie {
    leaf: Option<Leaf>,
    children: BTreeMap<OsString, Trie>,
}

impl Trie {
    fn insert(&mut self, relative: &Path, leaf: Leaf) {
        let mut node = self;
        for part in relative.components() {
            node = node
                .children
                .entry(part.as_os_str().to_owned())
                .or_default();
        }
        if node.leaf.is_none() {
            node.leaf = Some(leaf);
        }
    }

    /// Turns a folder of the trie into a node, folding a folder with a single child into that
    /// child so a chain of folders reads as one `a/b/c` tile.
    fn into_node(self, name: String, path: PathBuf) -> Option<Node> {
        if let Some(leaf) = self.leaf {
            return Some(Node {
                key: leaf.path.to_string_lossy().into_owned(),
                name,
                path: Some(leaf.path),
                bytes: leaf.bytes,
                items: 1,
                kind: Kind::Item(leaf.item),
                children: Vec::new(),
            });
        }
        let mut children: Vec<Node> = self
            .children
            .into_iter()
            .filter_map(|(part, trie)| {
                let child = path.join(&part);
                trie.into_node(part.to_string_lossy().into_owned(), child)
            })
            .collect();
        match children.len() {
            0 => None,
            1 => {
                let mut only = children.remove(0);
                only.name = format!("{name}/{}", only.name);
                Some(only)
            }
            _ => Some(container(
                path.to_string_lossy().into_owned(),
                name,
                Some(path),
                Kind::Folder,
                children,
            )),
        }
    }
}

fn container(
    key: String,
    name: String,
    path: Option<PathBuf>,
    kind: Kind,
    children: Vec<Node>,
) -> Node {
    Node {
        key,
        name,
        path,
        bytes: children.iter().map(|child| child.bytes).sum(),
        items: children.iter().map(|child| child.items).sum(),
        kind,
        children,
    }
}

/// A group of leaves under `base`, such as one scan root, as a node whose children are the
/// folders and items right below it.
fn group(key: String, name: String, base: &Path, leaves: Vec<Leaf>) -> Option<Node> {
    if leaves.is_empty() {
        return None;
    }
    let mut trie = Trie::default();
    for leaf in leaves {
        let relative = leaf
            .path
            .strip_prefix(base)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        trie.insert(&relative, leaf);
    }
    let children = if let Some(leaf) = trie.leaf.take() {
        let name = leaf.path.file_name().map_or_else(
            || display_path(&leaf.path),
            |name| name.to_string_lossy().into_owned(),
        );
        let path = leaf.path.clone();
        vec![
            Trie {
                leaf: Some(leaf),
                children: BTreeMap::new(),
            }
            .into_node(name, path)?,
        ]
    } else {
        trie.children
            .into_iter()
            .filter_map(|(part, child)| {
                let path = base.join(&part);
                child.into_node(part.to_string_lossy().into_owned(), path)
            })
            .collect()
    };
    Some(container(
        key,
        name,
        Some(base.to_path_buf()),
        Kind::Group,
        children,
    ))
}

/// The deepest folder that holds every path.
fn common_ancestor(paths: impl IntoIterator<Item = PathBuf>) -> PathBuf {
    let mut paths = paths.into_iter();
    let Some(first) = paths.next() else {
        return PathBuf::new();
    };
    let mut base = first.parent().map(Path::to_path_buf).unwrap_or(first);
    for path in paths {
        while !path.starts_with(&base) {
            if !base.pop() {
                return PathBuf::new();
            }
        }
    }
    base
}

/// Sorts every level largest first and folds the small tail of a level into one tile.
fn arrange(node: &mut Node) {
    for child in &mut node.children {
        arrange(child);
    }
    node.children
        .sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    #[allow(
        clippy::cast_precision_loss,
        reason = "a share only needs a few digits"
    )]
    let floor = node.bytes as f64 * MIN_SHARE;
    #[allow(
        clippy::cast_precision_loss,
        reason = "a share only needs a few digits"
    )]
    let cut = node
        .children
        .iter()
        .position(|child| (child.bytes as f64) < floor)
        .unwrap_or(node.children.len())
        .min(MAX_TILES - 1);
    if node.children.len() - cut >= 2 {
        let rest: Vec<Node> = node.children.drain(cut..).collect();
        let count = rest.len();
        node.children.push(Node {
            key: format!("{}{REST}", node.key),
            name: format!("{count} smaller"),
            path: None,
            bytes: rest.iter().map(|child| child.bytes).sum(),
            items: rest.iter().map(|child| child.items).sum(),
            kind: Kind::Rest(count),
            children: rest,
        });
    }
}

/// @name build
/// @description Builds the space map's tree: one group per scan root holding the folders and
/// project artifacts below it, a group for the global caches, and a group for artifacts outside
/// every root. Sizes add up the path, a chain of single folders becomes one `a/b` tile, and a
/// level's tiny tiles are folded into one "n smaller" tile. With a single group, that group is
/// the top of the map.
///
/// @example
/// let tree = build(&report.roots, artifacts, caches);
pub fn build(roots: &[PathBuf], artifacts: Vec<Leaf>, caches: Vec<Leaf>) -> Node {
    let mut by_root: Vec<(&PathBuf, Vec<Leaf>)> =
        roots.iter().map(|root| (root, Vec::new())).collect();
    by_root.sort_by_key(|(root, _)| std::cmp::Reverse(root.components().count()));
    let mut elsewhere = Vec::new();
    for leaf in artifacts {
        match by_root
            .iter_mut()
            .find(|(root, _)| leaf.path.starts_with(root))
        {
            Some((_, leaves)) => leaves.push(leaf),
            None => elsewhere.push(leaf),
        }
    }
    let mut groups: Vec<Node> = by_root
        .into_iter()
        .filter_map(|(root, leaves)| {
            group(
                root.to_string_lossy().into_owned(),
                display_path(root),
                root,
                leaves,
            )
        })
        .collect();
    let base = common_ancestor(elsewhere.iter().map(|leaf| leaf.path.clone()));
    groups.extend(group(
        ELSEWHERE.to_owned(),
        "Outside the scanned folders".to_owned(),
        &base,
        elsewhere,
    ));
    let base = common_ancestor(caches.iter().map(|leaf| leaf.path.clone()));
    groups.extend(group(
        CACHES.to_owned(),
        "Global caches".to_owned(),
        &base,
        caches,
    ));
    let mut top = if groups.len() == 1 {
        groups.remove(0)
    } else {
        let name = match roots {
            [root] => display_path(root),
            _ => "Everything".to_owned(),
        };
        container(String::new(), name, None, Kind::Group, groups)
    };
    arrange(&mut top);
    top
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: u64, path: &str, bytes: u64) -> Leaf {
        Leaf {
            path: PathBuf::from(path),
            bytes,
            item: Item {
                source: Source::Artifact,
                id,
                category: Category::Dependencies,
                safety: Safety::Safe,
            },
        }
    }

    fn cache(id: u64, path: &str, bytes: u64) -> Leaf {
        let mut leaf = leaf(id, path, bytes);
        leaf.item.source = Source::Cache;
        leaf.item.category = Category::Cache;
        leaf
    }

    fn names(node: &Node) -> Vec<&str> {
        node.children
            .iter()
            .map(|child| child.name.as_str())
            .collect()
    }

    #[test]
    fn sizes_add_up_the_path() {
        let roots = [PathBuf::from("/p")];
        let tree = build(
            &roots,
            vec![
                leaf(1, "/p/team/a/node_modules", 300),
                leaf(2, "/p/team/b/node_modules", 200),
                leaf(3, "/p/team/b/dist", 150),
                leaf(4, "/p/solo/target", 500),
            ],
            Vec::new(),
        );
        assert_eq!(tree.key, "/p");
        assert_eq!(tree.kind, Kind::Group);
        assert_eq!(tree.bytes, 1150);
        assert_eq!(tree.items, 4);
        assert_eq!(names(&tree), ["team", "solo/target"]);
        let team = &tree.children[0];
        assert_eq!(team.bytes, 650);
        assert_eq!(team.kind, Kind::Folder);
        assert_eq!(team.key, "/p/team");
        assert!(team.opens());
        assert_eq!(names(team), ["b", "a/node_modules"]);
        let b = &team.children[0];
        assert_eq!(b.bytes, 350);
        assert_eq!(names(b), ["node_modules", "dist"]);
        let solo = &tree.children[1];
        assert_eq!(solo.key, "/p/solo/target");
        assert_eq!(solo.item().map(|item| item.id), Some(4));
        assert!(!solo.opens());
    }

    #[test]
    fn single_child_chains_collapse_into_one_tile() {
        let roots = [PathBuf::from("/p")];
        let tree = build(
            &roots,
            vec![
                leaf(1, "/p/deep/er/still/x/node_modules", 10),
                leaf(2, "/p/deep/er/still/y/node_modules", 10),
                leaf(3, "/p/other/node_modules", 30),
            ],
            Vec::new(),
        );
        assert_eq!(names(&tree), ["other/node_modules", "deep/er/still"]);
        let chain = &tree.children[1];
        assert_eq!(
            chain.key, "/p/deep/er/still",
            "the key is the deepest folder"
        );
        assert_eq!(chain.path.as_deref(), Some(Path::new("/p/deep/er/still")));
        assert_eq!(names(chain), ["x/node_modules", "y/node_modules"]);
    }

    #[test]
    fn tiny_items_are_grouped_per_level() {
        let roots = [PathBuf::from("/p")];
        let mut leaves = vec![leaf(1, "/p/big/node_modules", 100_000)];
        for index in 0..5 {
            leaves.push(leaf(
                10 + index,
                &format!("/p/tiny{index}/node_modules"),
                10,
            ));
        }
        let tree = build(&roots, leaves, Vec::new());
        assert_eq!(tree.children.len(), 2);
        let rest = &tree.children[1];
        assert_eq!(rest.kind, Kind::Rest(5));
        assert_eq!(rest.name, "5 smaller");
        assert_eq!(rest.bytes, 50);
        assert_eq!(rest.items, 5);
        assert!(!rest.opens());
        assert_eq!(tree.leaves().len(), 6, "grouped items still count");

        let many: Vec<Leaf> = (0..200)
            .map(|index| leaf(index, &format!("/p/p{index:03}/node_modules"), 1_000))
            .collect();
        let tree = build(&roots, many, Vec::new());
        assert_eq!(tree.children.len(), MAX_TILES);
        assert_eq!(
            tree.children[MAX_TILES - 1].kind,
            Kind::Rest(200 - (MAX_TILES - 1))
        );

        let one_small = vec![
            leaf(1, "/p/a/node_modules", 100_000),
            leaf(2, "/p/b/node_modules", 1),
        ];
        let tree = build(&roots, one_small, Vec::new());
        assert_eq!(
            names(&tree),
            ["a/node_modules", "b/node_modules"],
            "one is not a group"
        );
    }

    #[test]
    fn roots_and_caches_are_groups_of_their_own() {
        let roots = [PathBuf::from("/p"), PathBuf::from("/q")];
        let tree = build(
            &roots,
            vec![
                leaf(1, "/p/a/node_modules", 40),
                leaf(2, "/p/b/node_modules", 40),
                leaf(3, "/q/c/node_modules", 10),
                leaf(4, "/elsewhere/z/dist", 5),
            ],
            vec![
                cache(5, "/home/u/.cache/pip", 70),
                cache(6, "/home/u/.npm", 30),
            ],
        );
        assert_eq!(tree.key, "");
        assert_eq!(tree.name, "Everything");
        assert_eq!(tree.bytes, 195);
        assert_eq!(
            names(&tree),
            ["Global caches", "/p", "/q", "Outside the scanned folders"]
        );
        let caches = &tree.children[0];
        assert_eq!(caches.key, CACHES);
        assert_eq!(names(caches), [".cache/pip", ".npm"]);
        assert_eq!(caches.children[0].item().unwrap().source, Source::Cache);
        let trail = tree.trail(&["/p".to_owned(), "missing".to_owned()]);
        assert_eq!(trail.len(), 2, "the walk stops at a key that is gone");
        assert_eq!(trail[1].name, "/p");
    }

    #[test]
    fn an_item_holding_another_item_wins() {
        let roots = [PathBuf::from("/p")];
        let tree = build(
            &roots,
            vec![
                leaf(1, "/p/a/node_modules", 50),
                leaf(2, "/p/a/node_modules/x/node_modules", 20),
                leaf(3, "/p/b/dist", 10),
            ],
            Vec::new(),
        );
        assert_eq!(tree.bytes, 60, "nothing is counted twice");
        assert_eq!(tree.leaves().len(), 2);
    }

    #[test]
    fn the_same_report_builds_the_same_tree() {
        let roots = [PathBuf::from("/p")];
        let leaves = vec![
            leaf(1, "/p/a/node_modules", 10),
            leaf(2, "/p/b/node_modules", 10),
            leaf(3, "/p/c/node_modules", 10),
        ];
        let mut reversed = leaves.clone();
        reversed.reverse();
        let first = build(&roots, leaves, Vec::new());
        assert_eq!(first, build(&roots, reversed, Vec::new()));
        assert_eq!(
            names(&first),
            ["a/node_modules", "b/node_modules", "c/node_modules"]
        );
        assert!(build(&roots, Vec::new(), Vec::new()).children.is_empty());
    }
}

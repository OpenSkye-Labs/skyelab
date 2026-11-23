//! A terminal tab's panes as a split tree (side by side or stacked, nested), and the form a tab is saved in so it
//! reopens after a restart.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The sizes in one split add up to this (basis points, so the tree stays `Eq` and exact).
pub const WHOLE: u32 = 10_000;
/// No pane is dragged below this share of its split.
pub const MIN_SHARE: u32 = 1_000;

/// How a split lays out its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    /// Side by side (T3's horizontal split).
    Row,
    /// Stacked (T3's vertical split).
    Column,
}

/// Leaves are panes (`L` is whatever names one: a view id live, a folder when saved).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneTree<L> {
    Leaf(L),
    /// Two or more children; `sizes[i]` is child i's share, the shares add up to [`WHOLE`].
    Split {
        axis: Axis,
        children: Vec<PaneTree<L>>,
        sizes: Vec<u32>,
    },
}

impl<L: PartialEq + Clone> PaneTree<L> {
    /// The leaves in reading order (left to right, top to bottom).
    pub fn leaves(&self) -> Vec<&L> {
        match self {
            PaneTree::Leaf(l) => vec![l],
            PaneTree::Split { children, .. } => children.iter().flat_map(|c| c.leaves()).collect(),
        }
    }

    /// Puts `new` after `at` along `axis`: a sibling when `at`'s split already runs that way (taking half of `at`'s
    /// share), else `at` becomes a split of the two. False when `at` isn't in the tree.
    pub fn split(&mut self, at: &L, new: L, axis: Axis) -> bool {
        match self {
            PaneTree::Leaf(l) if l == at => {
                let old = PaneTree::Leaf(l.clone());
                *self = PaneTree::Split {
                    axis,
                    children: vec![old, PaneTree::Leaf(new)],
                    sizes: vec![WHOLE / 2, WHOLE - WHOLE / 2],
                };
                true
            }
            PaneTree::Leaf(_) => false,
            PaneTree::Split {
                axis: own,
                children,
                sizes,
            } => {
                let direct = children
                    .iter()
                    .position(|c| matches!(c, PaneTree::Leaf(l) if l == at));
                if let (Some(i), true) = (direct, *own == axis) {
                    let half = sizes[i] / 2;
                    sizes[i] -= half;
                    sizes.insert(i + 1, half);
                    children.insert(i + 1, PaneTree::Leaf(new));
                    return true;
                }
                // `new` moves into the first child that holds `at`; the others never see it.
                let mut new = Some(new);
                children.iter_mut().any(|c| {
                    c.contains(at)
                        && c.split(at, new.take().expect("only the first match splits"), axis)
                })
            }
        }
    }

    pub fn contains(&self, at: &L) -> bool {
        match self {
            PaneTree::Leaf(l) => l == at,
            PaneTree::Split { children, .. } => children.iter().any(|c| c.contains(at)),
        }
    }

    /// Takes `at` out; its share goes to the neighbour before it (after it when first), and a split left with one
    /// child becomes that child. `None` when `at` was the only pane (the tab closes), else the tree without it.
    pub fn remove(self, at: &L) -> Option<Self> {
        match self {
            PaneTree::Leaf(l) if &l == at => None,
            leaf @ PaneTree::Leaf(_) => Some(leaf),
            PaneTree::Split {
                axis,
                children,
                mut sizes,
            } => {
                let mut kept = Vec::with_capacity(children.len());
                let mut kept_sizes = Vec::with_capacity(children.len());
                let mut freed = 0;
                for (child, size) in children.into_iter().zip(sizes.drain(..)) {
                    match child.remove(at) {
                        Some(c) => {
                            kept.push(c);
                            kept_sizes.push(size);
                        }
                        None => freed = size,
                    }
                    // A pane gone from the middle: its share joins the one before it.
                    if freed > 0 && !kept_sizes.is_empty() {
                        *kept_sizes.last_mut().expect("checked non-empty") += freed;
                        freed = 0;
                    }
                }
                if freed > 0 {
                    // It was the first child: the next one (now first) takes the share.
                    if let Some(first) = kept_sizes.first_mut() {
                        *first += freed;
                    }
                }
                match kept.len() {
                    0 => None,
                    1 => kept.pop(),
                    _ => Some(PaneTree::Split {
                        axis,
                        children: kept,
                        sizes: kept_sizes,
                    }),
                }
            }
        }
    }

    /// The split at `path` (child indexes from the root): the divider after child `divider` moves to `at`, a share of
    /// the whole split (0.0 = its start). Both sides keep at least [`MIN_SHARE`]; the panes beyond them stay.
    pub fn resize(&mut self, path: &[usize], divider: usize, at: f32) -> bool {
        let Some(PaneTree::Split {
            children, sizes, ..
        }) = self.node_mut(path)
        else {
            return false;
        };
        if divider + 1 >= children.len() {
            return false;
        }
        let before: u32 = sizes[..divider].iter().sum();
        let pair = sizes[divider] + sizes[divider + 1];
        let want = (at.clamp(0., 1.) * WHOLE as f32).round() as u32;
        let first = want
            .saturating_sub(before)
            .clamp(MIN_SHARE.min(pair / 2), pair - MIN_SHARE.min(pair / 2));
        sizes[divider] = first;
        sizes[divider + 1] = pair - first;
        true
    }

    fn node_mut(&mut self, path: &[usize]) -> Option<&mut Self> {
        match path.split_first() {
            None => Some(self),
            Some((i, rest)) => match self {
                PaneTree::Split { children, .. } => children.get_mut(*i)?.node_mut(rest),
                PaneTree::Leaf(_) => None,
            },
        }
    }

    /// The same shape with each leaf mapped (live view ids → saved folders and back).
    pub fn map<M>(&self, f: &mut impl FnMut(&L) -> M) -> PaneTree<M> {
        match self {
            PaneTree::Leaf(l) => PaneTree::Leaf(f(l)),
            PaneTree::Split {
                axis,
                children,
                sizes,
            } => PaneTree::Split {
                axis: *axis,
                children: children.iter().map(|c| c.map(f)).collect(),
                sizes: sizes.clone(),
            },
        }
    }

    /// A tree read from a file made safe to draw: sizes that don't match their children or don't add up become
    /// equal shares, and a split of one child becomes that child. `None` for a split with no children.
    pub fn repaired(self) -> Option<Self> {
        match self {
            leaf @ PaneTree::Leaf(_) => Some(leaf),
            PaneTree::Split {
                axis,
                children,
                sizes,
            } => {
                let mut children: Vec<_> =
                    children.into_iter().filter_map(Self::repaired).collect();
                match children.len() {
                    0 => None,
                    1 => children.pop(),
                    n => {
                        let ok = sizes.len() == n
                            && sizes.iter().all(|s| *s > 0)
                            && sizes.iter().sum::<u32>() == WHOLE;
                        let sizes = if ok { sizes } else { equal(n) };
                        Some(PaneTree::Split {
                            axis,
                            children,
                            sizes,
                        })
                    }
                }
            }
        }
    }
}

fn equal(n: usize) -> Vec<u32> {
    let n32 = n as u32;
    (0..n32)
        .map(|i| WHOLE / n32 + u32::from(i < WHOLE % n32))
        .collect()
}

/// One terminal tab as saved with its window: which thread it goes with, the name the user gave it, and each pane's
/// last working folder. The shells themselves don't survive a restart; new ones start in those folders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedTerminalTab {
    /// The store id of the thread it shows with; `None` shows with every thread.
    #[serde(default)]
    pub thread: Option<i64>,
    /// A name the user (or a project script) gave the tab; unset follows the running command.
    #[serde(default)]
    pub name: Option<String>,
    pub panes: PaneTree<PathBuf>,
}

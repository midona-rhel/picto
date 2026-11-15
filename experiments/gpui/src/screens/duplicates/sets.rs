//! Duplicate sets: the library reports pairs of matching files; files that
//! match each other, directly or through another file, are reviewed
//! together as one set and merged into the one file kept. This module
//! builds the sets, reads the library's own suggestion of a file to keep
//! (`compare_candidate_quality`: the larger picture, the lossless format,
//! more bytes of the same format), and turns a decision on a set into the
//! pairs the library settles.
use picto_library::{
    DuplicateCandidate, DuplicateCandidateSide, DuplicateQualityDecision,
    DuplicateResolutionChoice, FileId,
};
use std::collections::{BTreeMap, HashMap};

/// One matching pair inside a set.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub a: FileId,
    pub b: FileId,
    /// How different the two pictures are, in hundredths of a percent.
    pub distance: u32,
    pub decision: DuplicateQualityDecision,
}

impl Edge {
    /// The file the library judges better, when the files differ in
    /// quality.
    fn winner(&self) -> Option<(FileId, FileId)> {
        match self.decision {
            DuplicateQualityDecision::LeftBetter => Some((self.a, self.b)),
            DuplicateQualityDecision::RightBetter => Some((self.b, self.a)),
            _ => None,
        }
    }

    /// The file Smart merge keeps of the two (`smartMergeWinner`): the
    /// better one, or of two the library finds equal the one it settles on,
    /// the older file.
    fn kept(&self) -> Option<(FileId, FileId)> {
        match self.decision {
            DuplicateQualityDecision::LeftBetter | DuplicateQualityDecision::AutoTieLeft => {
                Some((self.a, self.b))
            }
            DuplicateQualityDecision::RightBetter | DuplicateQualityDecision::AutoTieRight => {
                Some((self.b, self.a))
            }
            DuplicateQualityDecision::NeedsChoice => None,
        }
    }

    fn touches(&self, file: FileId) -> bool {
        self.a == file || self.b == file
    }
}

#[derive(Clone, Debug)]
pub struct Set {
    /// The set's smallest file id, which names it across reads.
    pub key: u32,
    /// Its files, oldest file first.
    pub files: Vec<DuplicateCandidateSide>,
    pub edges: Vec<Edge>,
    /// The file Smart merge keeps, when the library's rules settle on one
    /// file over every other. It is shown first and named as the
    /// suggestion; the choice stays the user's.
    pub suggested: Option<usize>,
}

/// The sets the pairs make: files linked by a pair, directly or through
/// other files, are one set. Sets come in the order their first pair came.
pub fn build(pairs: &[DuplicateCandidate]) -> Vec<Set> {
    let mut parent = HashMap::<u32, u32>::new();
    fn root(parent: &mut HashMap<u32, u32>, file: u32) -> u32 {
        let mut at = *parent.entry(file).or_insert(file);
        while parent[&at] != at {
            let up = parent[&parent[&at]];
            parent.insert(at, up);
            at = up;
        }
        at
    }
    for pair in pairs {
        let (a, b) = (
            root(&mut parent, pair.file_id_a.0),
            root(&mut parent, pair.file_id_b.0),
        );
        if a != b {
            parent.insert(a.max(b), a.min(b));
        }
    }
    let mut order = Vec::<u32>::new();
    let mut grouped = HashMap::<u32, (BTreeMap<u32, DuplicateCandidateSide>, Vec<Edge>)>::new();
    for pair in pairs {
        let at = root(&mut parent, pair.file_id_a.0);
        let entry = grouped.entry(at).or_insert_with(|| {
            order.push(at);
            Default::default()
        });
        entry
            .0
            .entry(pair.file_id_a.0)
            .or_insert_with(|| pair.left.clone());
        entry
            .0
            .entry(pair.file_id_b.0)
            .or_insert_with(|| pair.right.clone());
        entry.1.push(Edge {
            a: pair.file_id_a,
            b: pair.file_id_b,
            distance: pair.distance,
            decision: pair.decision,
        });
    }
    order
        .into_iter()
        .filter_map(|at| grouped.remove(&at))
        .map(|(files, edges)| {
            let files = files.into_values().collect::<Vec<_>>();
            let suggested = suggest(&files, &edges);
            Set {
                key: files[0].file.file_id.0,
                files,
                edges,
                suggested,
            }
        })
        .collect()
}

/// The one file Smart merge keeps in every pair it is in, when there is
/// exactly one and no pair leaves the choice open.
fn suggest(files: &[DuplicateCandidateSide], edges: &[Edge]) -> Option<usize> {
    if edges.iter().any(|edge| edge.kept().is_none()) {
        return None;
    }
    let merged_away = |file: FileId| {
        edges
            .iter()
            .any(|edge| edge.kept().is_some_and(|(_, loser)| loser == file))
    };
    let mut best = (0..files.len()).filter(|&index| !merged_away(files[index].file.file_id));
    match (best.next(), best.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

impl Set {
    /// Every pair has the same perceptual hash: identical to the eye of
    /// the scan, not known to be the same pixels.
    pub fn identical(&self) -> bool {
        self.edges.iter().all(|edge| edge.distance == 0)
    }

    /// No pair finds one file better: the library rates the files equal,
    /// and its suggestion is only the older file.
    pub fn equal_quality(&self) -> bool {
        self.edges.iter().all(|edge| edge.winner().is_none())
    }

    /// The least alike pair's distance.
    pub fn distance(&self) -> u32 {
        self.edges
            .iter()
            .map(|edge| edge.distance)
            .max()
            .unwrap_or(0)
    }

    pub fn index_of(&self, file: FileId) -> Option<usize> {
        self.files.iter().position(|side| side.file.file_id == file)
    }

    /// The pairs that merge every file into `keeper`: each file is merged
    /// into the one that links it toward the keeper, farthest first, so
    /// what a file carries reaches the keeper through the files between.
    pub fn merge_steps(&self, keeper: usize) -> Vec<(FileId, FileId, DuplicateResolutionChoice)> {
        let start = self.files[keeper].file.file_id;
        let mut reached = vec![start];
        let mut steps = Vec::new();
        let mut at = 0;
        while at < reached.len() {
            let file = reached[at];
            for edge in self.edges.iter().filter(|edge| edge.touches(file)) {
                let other = if edge.a == file { edge.b } else { edge.a };
                if !reached.contains(&other) {
                    reached.push(other);
                    steps.push((
                        edge.a,
                        edge.b,
                        DuplicateResolutionChoice::KeepFile {
                            winner_file_id: file,
                        },
                    ));
                }
            }
            at += 1;
        }
        steps.reverse();
        steps
    }

    /// The pairs that keep every file of the set apart.
    pub fn apart_steps(&self) -> Vec<(FileId, FileId, DuplicateResolutionChoice)> {
        self.edges
            .iter()
            .map(|edge| (edge.a, edge.b, DuplicateResolutionChoice::KeepBoth))
            .collect()
    }

    /// The pairs that take one file out of the set.
    pub fn without_steps(&self, file: usize) -> Vec<(FileId, FileId, DuplicateResolutionChoice)> {
        let id = self.files[file].file.file_id;
        self.edges
            .iter()
            .filter(|edge| edge.touches(id))
            .map(|edge| (edge.a, edge.b, DuplicateResolutionChoice::KeepBoth))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::build;
    use picto_library::{
        DuplicateCandidate, DuplicateCandidateSide, DuplicateFile, DuplicateQualityDecision,
        DuplicateResolutionChoice, FileId,
    };

    fn side(id: u32, width: u32) -> DuplicateCandidateSide {
        DuplicateCandidateSide {
            file: DuplicateFile {
                file_id: FileId(id),
                file_hash: format!("{id:064}"),
                mime_type: "image/png".into(),
                size_bytes: 1000 * id as i64,
                pixel_width: Some(width),
                pixel_height: Some(width),
                frame_count: None,
            },
            occurrences: Vec::new(),
        }
    }

    fn pair(
        a: (u32, u32),
        b: (u32, u32),
        distance: u32,
        decision: DuplicateQualityDecision,
    ) -> DuplicateCandidate {
        DuplicateCandidate {
            file_id_a: FileId(a.0),
            file_id_b: FileId(b.0),
            distance,
            left: side(a.0, a.1),
            right: side(b.0, b.1),
            decision,
        }
    }

    use DuplicateQualityDecision::*;

    #[test]
    fn files_linked_through_another_file_are_one_set() {
        let sets = build(&[
            pair((5, 100), (9, 100), 0, AutoTieLeft),
            pair((1, 100), (2, 100), 0, AutoTieLeft),
            // 9 matches 7 too, so 5, 7 and 9 are one set.
            pair((7, 100), (9, 100), 0, AutoTieLeft),
        ]);
        assert_eq!(sets.len(), 2);
        let ids = |index: usize| {
            sets[index]
                .files
                .iter()
                .map(|side| side.file.file_id.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(0), [5, 7, 9]);
        assert_eq!(ids(1), [1, 2]);
        assert_eq!(sets[0].key, 5);
        assert_eq!(sets[0].edges.len(), 2);
    }

    #[test]
    fn the_library_suggests_the_one_file_better_than_every_other() {
        // 3 beats 2, 2 beats 1: 3 is suggested.
        let sets = build(&[
            pair((1, 100), (2, 200), 1, RightBetter),
            pair((2, 200), (3, 300), 1, RightBetter),
        ]);
        assert_eq!(
            sets[0].suggested.map(|at| sets[0].files[at].file.file_id),
            Some(FileId(3))
        );
        // Of two files the library finds equal, the one it settles on.
        let sets = build(&[pair((4, 100), (6, 100), 0, AutoTieLeft)]);
        assert_eq!(sets[0].suggested, Some(0));
        assert!(sets[0].equal_quality());
    }

    #[test]
    fn two_unbeaten_files_or_an_open_choice_suggest_nothing() {
        // 1 and 3 both beat 2, and nothing compares 1 with 3.
        let sets = build(&[
            pair((1, 300), (2, 100), 40, LeftBetter),
            pair((2, 100), (3, 200), 40, RightBetter),
        ]);
        assert_eq!(sets[0].suggested, None);
        let sets = build(&[pair((1, 300), (2, 100), 40, NeedsChoice)]);
        assert_eq!(sets[0].suggested, None);
    }

    #[test]
    fn a_merge_folds_the_farthest_files_in_first() {
        // A chain 1 - 2 - 3 kept at 1: 3 goes into 2, then 2 into 1.
        let sets = build(&[
            pair((1, 100), (2, 100), 0, AutoTieLeft),
            pair((2, 100), (3, 100), 0, AutoTieLeft),
        ]);
        let steps = sets[0].merge_steps(0);
        let keep = |id| DuplicateResolutionChoice::KeepFile {
            winner_file_id: FileId(id),
        };
        assert_eq!(
            steps,
            [
                (FileId(2), FileId(3), keep(2)),
                (FileId(1), FileId(2), keep(1)),
            ]
        );
        // Kept at the far end, the chain folds the other way.
        assert_eq!(
            sets[0].merge_steps(2),
            [
                (FileId(1), FileId(2), keep(2)),
                (FileId(2), FileId(3), keep(3)),
            ]
        );
    }
}

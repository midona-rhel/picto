//! Duplicate sets: the library reports pairs of matching files; files that
//! match each other, directly or through another file, are reviewed
//! together as one set with one file to keep. This module builds the sets,
//! chooses the file Smart merge keeps (`smartMergeWinner`, extended from a
//! pair to a set), and turns a decision on a set into the pairs the library
//! settles.
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
    /// The file the library judges better, when it can tell; a tie it
    /// settles in a stable order counts, as it does for Smart merge.
    fn winner(&self) -> Option<(FileId, FileId)> {
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

/// How sure the library is of the file to keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Every pair is the same picture.
    Identical,
    /// Similar pictures, one of them better than every other.
    Clear,
    /// Similar pictures and no clear best: the user looks first.
    Unsure,
}

#[derive(Clone, Debug)]
pub struct Set {
    /// The set's smallest file id, which names it across reads.
    pub key: u32,
    /// Its files, oldest file first.
    pub files: Vec<DuplicateCandidateSide>,
    pub edges: Vec<Edge>,
    pub verdict: Verdict,
    /// The file Smart merge keeps, or the largest when it cannot tell.
    pub best: usize,
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
            let (verdict, best) = judge(&files, &edges);
            Set {
                key: files[0].file.file_id.0,
                files,
                edges,
                verdict,
                best,
            }
        })
        .collect()
}

/// The verdict and the file to keep: the one file no pair finds worse than
/// another, when there is exactly one and no pair needs a choice; else the
/// largest picture, then the largest file.
fn judge(files: &[DuplicateCandidateSide], edges: &[Edge]) -> (Verdict, usize) {
    let beaten = |file: FileId| {
        edges
            .iter()
            .any(|edge| edge.winner().is_some_and(|(_, loser)| loser == file))
    };
    let unbeaten = (0..files.len())
        .filter(|&index| !beaten(files[index].file.file_id))
        .collect::<Vec<_>>();
    let undecided = edges
        .iter()
        .any(|edge| edge.decision == DuplicateQualityDecision::NeedsChoice);
    let identical = edges.iter().all(|edge| edge.distance == 0);
    if let [only] = unbeaten[..]
        && !undecided
    {
        let verdict = if identical {
            Verdict::Identical
        } else {
            Verdict::Clear
        };
        return (verdict, only);
    }
    let largest = (0..files.len())
        .max_by_key(|&index| {
            let file = &files[index].file;
            let pixels =
                file.pixel_width.unwrap_or(0) as u64 * file.pixel_height.unwrap_or(0) as u64;
            (pixels, file.size_bytes, std::cmp::Reverse(file.file_id.0))
        })
        .unwrap_or(0);
    let verdict = if identical {
        Verdict::Identical
    } else {
        Verdict::Unsure
    };
    (verdict, largest)
}

impl Set {
    pub fn identical(&self) -> bool {
        self.verdict == Verdict::Identical
    }

    /// The least alike pair's distance.
    pub fn distance(&self) -> u32 {
        self.edges
            .iter()
            .map(|edge| edge.distance)
            .max()
            .unwrap_or(0)
    }

    /// The bytes merging frees: every file but the one kept.
    pub fn saving(&self, keeper: usize) -> i64 {
        self.files
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != keeper)
            .map(|(_, side)| side.file.size_bytes.max(0))
            .sum()
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

/// How the shown file compares with the rest of its set, for Compare's
/// statements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lean {
    Better,
    Worse,
    Same,
}

/// The shown file's area against the largest of the others: its share, in
/// percent, rounded.
pub fn resolution_share(set: &Set, shown: usize) -> Option<u32> {
    let area = |index: usize| {
        let file = &set.files[index].file;
        Some(file.pixel_width? as f64 * file.pixel_height? as f64)
    };
    let mine = area(shown)?;
    let others = (0..set.files.len())
        .filter(|index| *index != shown)
        .map(area)
        .collect::<Option<Vec<_>>>()?;
    let most = others.into_iter().fold(0., f64::max);
    (most > 0.).then(|| (mine / most * 100.).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::{Verdict, build};
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
    fn the_file_no_pair_finds_worse_is_kept() {
        // 3 beats 2, 2 beats 1: 3 is clearly best.
        let sets = build(&[
            pair((1, 100), (2, 200), 1, RightBetter),
            pair((2, 200), (3, 300), 1, RightBetter),
        ]);
        assert_eq!(sets[0].verdict, Verdict::Clear);
        assert_eq!(sets[0].files[sets[0].best].file.file_id, FileId(3));
        // Identical pictures keep the core's stable choice.
        let sets = build(&[pair((4, 100), (6, 100), 0, AutoTieLeft)]);
        assert_eq!(sets[0].verdict, Verdict::Identical);
        assert_eq!(sets[0].best, 0);
    }

    #[test]
    fn two_unbeaten_files_or_an_open_choice_leave_the_set_unsure() {
        // 1 and 3 both beat 2, and nothing compares 1 with 3.
        let sets = build(&[
            pair((1, 300), (2, 100), 40, LeftBetter),
            pair((2, 100), (3, 200), 40, RightBetter),
        ]);
        assert_eq!(sets[0].verdict, Verdict::Unsure);
        // The largest picture is offered.
        assert_eq!(sets[0].files[sets[0].best].file.file_id, FileId(1));
        let sets = build(&[pair((1, 300), (2, 100), 40, NeedsChoice)]);
        assert_eq!(sets[0].verdict, Verdict::Unsure);
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
        assert_eq!(sets[0].saving(0), 2000 + 3000);
    }
}

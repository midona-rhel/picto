//! The stacked preview of a multiple selection (`StackedPreview` in
//! `Inspector.tsx`): which cards are in the stack, the pose each keeps for as
//! long as the stack is shown, their order, their shade, and the enter and
//! exit motion.
use crate::motion;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The stack shows the last six of the sample.
pub const CARDS: usize = 6;
/// How dark the bottom card is under the rail-coloured shade.
const DEEPEST_SHADE: f32 = 0.7;
/// The scale of a turned card (`.stackItemLeft`, `.stackItemRight`).
const TURNED_SCALE: f32 = 0.93;
/// Where a card enters from, and how large it starts (`@keyframes
/// stackPreviewLeft`, `stackPreviewRight`).
const ENTER_FROM: (f32, f32) = (100., 40.);
const ENTER_SCALE: f32 = 1.2;
const ENTER_LEFT: Duration = Duration::from_millis(200);
const ENTER_RIGHT: Duration = Duration::from_millis(250);
/// A card that leaves fades over 200 ms and is gone at 220 ms.
const EXIT_FADE: Duration = Duration::from_millis(200);
const EXIT_REMOVE: Duration = Duration::from_millis(220);

/// A card's resting pose. The first card lies straight; the rest alternate,
/// turned five degrees one way or the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Base,
    Left,
    Right,
}

impl Pose {
    fn nth(sequence: usize) -> Self {
        match sequence {
            0 => Pose::Base,
            n if n % 2 == 0 => Pose::Left,
            _ => Pose::Right,
        }
    }
}

#[derive(Clone, Debug)]
struct Card {
    hash: String,
    pose: Pose,
    /// Paint order among the cards still in the stack: later is higher.
    z: u64,
    entered: Option<Instant>,
    exited: Option<Instant>,
    /// The shade, and where it was fading from and when (200 ms).
    shade: f32,
    shade_from: f32,
    shade_at: Option<Instant>,
}

/// How to draw one card at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub hash: String,
    pub pose: Pose,
    /// Offset from the resting place, in pixels.
    pub offset: (f32, f32),
    pub scale: f32,
    pub opacity: f32,
    pub shade: f32,
}

#[derive(Default)]
pub struct Stack {
    poses: HashMap<String, Pose>,
    next_pose: usize,
    next_z: u64,
    cards: Vec<Card>,
    shown: Vec<String>,
}

fn shade_at(index: usize, count: usize) -> f32 {
    if count <= 1 {
        0.
    } else {
        DEEPEST_SHADE * (1. - index as f32 / (count - 1) as f32)
    }
}

impl Stack {
    /// Puts the last six of `sample` in the stack. Cards already in it keep
    /// their pose and place; new cards enter on top; one card that left
    /// fades out under the rest.
    pub fn show(&mut self, sample: &[String], now: Instant) {
        let previews = &sample[sample.len().saturating_sub(CARDS)..];
        if previews == self.shown.as_slice() {
            return;
        }
        let first = self.shown.is_empty() && self.cards.is_empty();
        let count = previews.len();
        let mut next = Vec::with_capacity(count + 1);
        for (index, hash) in previews.iter().enumerate() {
            let shade = shade_at(index, count);
            // A card on its way out comes back where it was.
            if let Some(existing) = self.cards.iter().find(|card| card.hash == *hash) {
                let mut card = existing.clone();
                card.exited = None;
                if (card.shade - shade).abs() > f32::EPSILON {
                    card.shade_from = card.shade_now(now);
                    card.shade_at = Some(now);
                    card.shade = shade;
                }
                next.push(card);
                continue;
            }
            let pose = *self.poses.entry(hash.clone()).or_insert_with(|| {
                let pose = Pose::nth(self.next_pose);
                self.next_pose += 1;
                pose
            });
            // The stack's first frame animates only its top card in.
            let entering = if first {
                count > 1 && index == count - 1
            } else {
                true
            };
            next.push(Card {
                hash: hash.clone(),
                pose,
                z: self.next_z,
                entered: entering.then_some(now),
                exited: None,
                shade,
                shade_from: shade,
                shade_at: None,
            });
            self.next_z += 1;
        }
        let leaving = self
            .cards
            .iter()
            .rfind(|card| card.exited.is_none() && !previews.contains(&card.hash))
            .cloned()
            .map(|mut card| {
                card.exited = Some(now);
                card
            });
        next.extend(leaving);
        self.cards = next;
        self.shown = previews.to_vec();
    }

    /// Whether the stack has never shown a card since it started.
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty() && self.shown.is_empty()
    }

    /// The cards to draw, bottom first, at `now`.
    pub fn placed(&mut self, now: Instant) -> Vec<Placed> {
        self.cards.retain(|card| {
            card.exited
                .is_none_or(|exited| now.duration_since(exited) < EXIT_REMOVE)
        });
        let mut cards = self.cards.iter().collect::<Vec<_>>();
        // A leaving card goes under every other; the rest stack by age.
        cards.sort_by_key(|card| (card.exited.is_none(), card.z));
        cards
            .into_iter()
            .map(|card| {
                let rest = if card.pose == Pose::Base {
                    1.
                } else {
                    TURNED_SCALE
                };
                let (mut offset, mut scale, mut opacity) = ((0., 0.), rest, 1.);
                if let (Some(entered), false) = (card.entered, card.pose == Pose::Base) {
                    let duration = if card.pose == Pose::Left {
                        ENTER_LEFT
                    } else {
                        ENTER_RIGHT
                    };
                    let progress = motion::EASE_DEFAULT
                        .ease(motion::progress(now.duration_since(entered), duration));
                    offset = (
                        ENTER_FROM.0 * (1. - progress),
                        ENTER_FROM.1 * (1. - progress),
                    );
                    scale = ENTER_SCALE + (rest - ENTER_SCALE) * progress;
                    opacity = progress;
                }
                if let Some(exited) = card.exited {
                    opacity *= 1.
                        - motion::EASE_DEFAULT
                            .ease(motion::progress(now.duration_since(exited), EXIT_FADE));
                }
                Placed {
                    hash: card.hash.clone(),
                    pose: card.pose,
                    offset,
                    scale,
                    opacity,
                    shade: card.shade_now(now),
                }
            })
            .collect()
    }

    /// Whether a card is still moving, fading or changing shade.
    pub fn animating(&self, now: Instant) -> bool {
        self.cards.iter().any(|card| {
            let entering = card.pose != Pose::Base
                && card
                    .entered
                    .is_some_and(|at| now.duration_since(at) < ENTER_RIGHT);
            let shading = card
                .shade_at
                .is_some_and(|at| now.duration_since(at) < motion::NORMAL);
            entering || shading || card.exited.is_some()
        })
    }
}

impl Card {
    fn shade_now(&self, now: Instant) -> f32 {
        match self.shade_at {
            Some(at) => {
                let progress = motion::EASE_DEFAULT
                    .ease(motion::progress(now.duration_since(at), motion::NORMAL));
                self.shade_from + (self.shade - self.shade_from) * progress
            }
            None => self.shade,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Placed, Pose, Stack};
    use std::time::{Duration, Instant};

    fn hashes(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn settled(stack: &mut Stack, at: Instant) -> Vec<Placed> {
        stack.placed(at + Duration::from_secs(1))
    }

    #[test]
    fn the_first_card_lies_straight_and_the_rest_alternate() {
        let mut stack = Stack::default();
        let now = Instant::now();
        stack.show(&hashes(&["a", "b", "c", "d"]), now);
        let poses = settled(&mut stack, now)
            .into_iter()
            .map(|card| card.pose)
            .collect::<Vec<_>>();
        assert_eq!(poses, [Pose::Base, Pose::Right, Pose::Left, Pose::Right]);
    }

    #[test]
    fn the_newest_card_is_unshaded_and_the_oldest_deepest() {
        let mut stack = Stack::default();
        let now = Instant::now();
        stack.show(&hashes(&["a", "b", "c"]), now);
        let shades = settled(&mut stack, now)
            .into_iter()
            .map(|card| card.shade)
            .collect::<Vec<_>>();
        assert!((shades[0] - 0.7).abs() < 1e-6);
        assert!((shades[1] - 0.35).abs() < 1e-6);
        assert_eq!(shades[2], 0.);
    }

    #[test]
    fn only_the_last_six_are_stacked() {
        let mut stack = Stack::default();
        let now = Instant::now();
        let sample = hashes(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        stack.show(&sample, now);
        let shown = settled(&mut stack, now)
            .into_iter()
            .map(|card| card.hash)
            .collect::<Vec<_>>();
        assert_eq!(shown, hashes(&["c", "d", "e", "f", "g", "h"]));
    }

    #[test]
    fn a_card_keeps_its_pose_and_a_new_one_enters_from_below_right() {
        let mut stack = Stack::default();
        let now = Instant::now();
        stack.show(&hashes(&["a", "b"]), now);
        let later = now + Duration::from_secs(1);
        stack.show(&hashes(&["a", "b", "c"]), later);
        let start = stack.placed(later);
        let entering = start.iter().find(|card| card.hash == "c").unwrap();
        assert_eq!(entering.pose, Pose::Left);
        assert_eq!(entering.offset, (100., 40.));
        assert_eq!(entering.opacity, 0.);
        assert!((entering.scale - 1.2).abs() < 1e-6);
        assert_eq!(start.last().unwrap().hash, "c");
        let rest = settled(&mut stack, later);
        let entered = rest.iter().find(|card| card.hash == "c").unwrap();
        assert_eq!(entered.offset, (0., 0.));
        assert!((entered.scale - 0.93).abs() < 1e-6);
        assert_eq!(rest[1].pose, Pose::Right);
    }

    #[test]
    fn a_card_that_leaves_fades_under_the_others_and_is_removed() {
        let mut stack = Stack::default();
        let now = Instant::now();
        stack.show(&hashes(&["a", "b", "c"]), now);
        stack.show(&hashes(&["a", "b"]), now);
        let placed = stack.placed(now);
        assert_eq!(placed[0].hash, "c");
        assert_eq!(placed.len(), 3);
        assert!(stack.animating(now));
        let gone = stack.placed(now + Duration::from_millis(220));
        assert_eq!(gone.len(), 2);
        assert!(!stack.animating(now + Duration::from_millis(220)));
    }
}

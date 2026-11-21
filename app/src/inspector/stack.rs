//! Holds the state of the stacked preview, which the inspector shows for a
//! selection of several items: which cards are in the stack, the pose each
//! card keeps for as long as the stack is shown, the order the cards are
//! drawn in, the shade over each card, and how a card moves when it enters
//! or leaves the stack.
use crate::motion;
use std::collections::HashMap;
use std::time::Instant;

/// The largest number of cards in the stack. The stack shows the last six
/// hashes of the sample given to [`Stack::show`].
pub const CARDS: usize = 6;
/// The opacity of the shade over the bottom card. The shade is a layer in
/// the rail's colour that is drawn over a card. The top card has none.
const DEEPEST_SHADE: f32 = 0.7;
/// The scale a turned card is drawn at when it is not moving.
const TURNED_SCALE: f32 = 0.93;
/// A card that enters the stack starts `ENTER_FROM` pixels to the right of
/// and below its resting place, at the scale `ENTER_SCALE`.
const ENTER_FROM: (f32, f32) = (100., 40.);
const ENTER_SCALE: f32 = 1.2;

/// How a card lies in the stack when it is not moving. The first card is not
/// turned. Each later card is turned five degrees: the second clockwise
/// (`Right`), the third anticlockwise (`Left`), and so on in turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    Base,
    Left,
    Right,
}

impl Pose {
    /// Returns how many degrees the card is turned clockwise when it is not
    /// moving.
    pub fn degrees(self) -> f32 {
        match self {
            Pose::Base => 0.,
            Pose::Left => -5.,
            Pose::Right => 5.,
        }
    }

    /// Returns the scale the card is drawn at when it is not moving.
    pub fn scale(self) -> f32 {
        if self == Pose::Base { 1. } else { TURNED_SCALE }
    }

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
    /// The drawing order among the cards that have not left the stack. A card
    /// with a larger value is drawn over a card with a smaller one.
    z: u64,
    entered: Option<Instant>,
    exited: Option<Instant>,
    /// The opacity of the shade that the card is changing to. `shade_from` is
    /// the opacity it is changing from and `shade_at` is when the change
    /// began. The change takes `motion::NORMAL`.
    shade: f32,
    shade_from: f32,
    shade_at: Option<Instant>,
}

/// Holds what is needed to draw one card at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub hash: String,
    pub pose: Pose,
    /// How far the card is from its resting place, in pixels to the right and
    /// down.
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
    /// Puts the last six hashes of `sample` in the stack. A card that is
    /// already in the stack keeps its pose and its drawing order. A new card
    /// is drawn over the others and moves in. Of the cards that are no longer
    /// in `sample`, the one that came last in the stack fades out under the
    /// others, and the rest are removed at once.
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
            // A card that is already in the stack, also one that is fading out,
            // keeps its pose and drawing order, and it no longer fades out.
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
            // When the stack is shown for the first time, only its top card moves
            // in, and only if there is more than one card. The other cards are
            // drawn at their resting places at once.
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

    /// Returns true if no card is in the stack, also none that is fading out.
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty() && self.shown.is_empty()
    }

    /// Returns the cards to draw at `now`, in drawing order with the bottom
    /// card first. Removes the cards that have finished fading out.
    pub fn placed(&mut self, now: Instant) -> Vec<Placed> {
        self.cards.retain(|card| {
            card.exited
                .is_none_or(|exited| now.duration_since(exited) < motion::STACK_EXIT_REMOVE)
        });
        let mut cards = self.cards.iter().collect::<Vec<_>>();
        // A card that is fading out is drawn under all the others. The others
        // are drawn in the order in which they entered the stack.
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
                        motion::STACK_ENTER_LEFT
                    } else {
                        motion::STACK_ENTER_RIGHT
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
                        - motion::EASE_DEFAULT.ease(motion::progress(
                            now.duration_since(exited),
                            motion::STACK_EXIT_FADE,
                        ));
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

    /// Returns true if at `now` a card is still moving in, fading out or
    /// changing its shade.
    pub fn animating(&self, now: Instant) -> bool {
        self.cards.iter().any(|card| {
            let entering = card.pose != Pose::Base
                && card
                    .entered
                    .is_some_and(|at| now.duration_since(at) < motion::STACK_ENTER_RIGHT);
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

//! The verbatim check of the shared data's build: a summary that repeats a player's
//! review word for word isn't UpNext's own text, so it isn't published. In a
//! sample of a few hundred titles, about one summary in thirty had a run of 8
//! or more words copied from a review; the build asks the model once more for
//! those, in its own words, and the export leaves out any that still copy.

use std::collections::HashSet;

/// A run of this many words copied from a review keeps a summary out.
pub const COPIED_WORDS: usize = 8;

/// A text's words, lower-cased, with the punctuation around them dropped,
/// so "Great, co-op puzzles!" and "great co-op puzzles" read the same.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()).filter(|w| !w.is_empty()).collect()
}

/// The longest run of words in `text` that a review also has, counted when
/// it is at least [`COPIED_WORDS`] long, else 0. Every window of that many
/// words is looked up in the reviews' windows, and touching windows add up:
/// two copied runs that overlap count as one longer run, which can only
/// overstate the copying.
pub fn longest_copied<'a>(text: &str, reviews: impl IntoIterator<Item = &'a str>) -> usize {
    let n = COPIED_WORDS;
    let mut windows: HashSet<Vec<String>> = HashSet::new();
    for review in reviews {
        let w = words(review);
        for at in 0..w.len().saturating_sub(n - 1) {
            windows.insert(w[at..at + n].to_vec());
        }
    }
    let w = words(text);
    let (mut longest, mut run) = (0, 0);
    for at in 0..w.len().saturating_sub(n - 1) {
        if windows.contains(&w[at..at + n]) {
            run = if run == 0 { n } else { run + 1 };
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;

    const REVIEW: &str = "Honestly the best co-op puzzle game I have ever played with a friend, and the writing is sharp.";

    #[test]
    fn a_planted_copy_is_found() {
        let copied = "Two robots solve rooms together. It is the best co-op puzzle game I have ever played, says everyone.";
        assert_eq!(longest_copied(copied, [REVIEW]), 9, "\"the best co-op puzzle game i have ever played\"");
        let punctuated = "THE BEST CO-OP PUZZLE GAME, I HAVE EVER PLAYED!";
        assert_eq!(longest_copied(punctuated, [REVIEW]), 9, "case and punctuation don't hide it");
    }

    #[test]
    fn a_clean_summary_passes() {
        let own = "Two robots trade wisecracks while you bend space with portals, and every room is a small, smug eureka.";
        assert_eq!(longest_copied(own, [REVIEW]), 0);
        let seven = "It is the best co-op puzzle game around.";
        assert_eq!(longest_copied(seven, [REVIEW]), 0, "seven words in a row are allowed");
        assert_eq!(longest_copied(REVIEW, std::iter::empty()), 0, "no reviews, nothing copied");
        assert_eq!(longest_copied("", [REVIEW]), 0);
    }

    #[test]
    fn several_reviews() {
        let other = "The soundtrack slaps and the boss fights are tense from start to finish.";
        let text = "A tense ride: the boss fights are tense from start to finish, honestly.";
        assert_eq!(longest_copied(text, [REVIEW, other]), 9, "\"the boss fights are tense from start to finish\"");
    }
}

//! Game names: the keys that group a game's copies into one title, and the
//! name clean-up and look-alike check that the sources which look a game up
//! by name (HowLongToBeat, IGDB, RAWG, the Steam store, PCGamingWiki and
//! Wikidata) use, so every lookup and the grouping read a name the same way.

/// Conservative name normalization for grouping copies of the same game:
/// case, trademark glyphs, punctuation and whitespace only. Edition words are
/// kept on purpose ("Alan Wake" and "Alan Wake Remastered" stay separate).
pub fn name_key(name: &str) -> String {
    let mut lower = name.trim().to_lowercase();
    // Library-style "Elder Scrolls IV: Oblivion, The" sorts the article last.
    for article in ["the", "a", "an"] {
        if let Some(rest) = lower.strip_suffix(&format!(", {article}")) {
            lower = format!("{article} {rest}");
            break;
        }
    }
    let cleaned: String = lower.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Edition-insensitive key: like `name_key` but with trailing edition
/// phrases removed ("X: Definitive Edition", "X GOTY", "X Remastered",
/// "X Director's Cut", "X (Enhanced)"). Applied repeatedly.
pub fn loose_key(name: &str) -> String {
    let mut s = name_key(name);
    let words = [
        "game of the year edition",
        "game of the year",
        "goty edition",
        "goty",
        "definitive edition",
        "complete edition",
        "enhanced edition",
        "special edition",
        "deluxe edition",
        "digital deluxe edition",
        "anniversary edition",
        "ultimate edition",
        "gold edition",
        "legendary edition",
        "premium edition",
        "extended edition",
        "standard edition",
        "steam edition",
        "gog edition",
        "epic edition",
        "directors cut",
        "director s cut",
        "remastered",
        "remaster",
        "enhanced",
        "definitive",
        "complete",
        "ultimate",
        "deluxe",
        "collection",
        "edition",
        "hd",
    ];
    loop {
        let before = s.clone();
        for w in words {
            if let Some(stripped) = s.strip_suffix(w) {
                s = stripped.trim_end_matches([' ', '-', ':']).trim().to_string();
            }
        }
        if s == before || s.is_empty() {
            break;
        }
    }
    if s.is_empty() { name_key(name) } else { s }
}

/// Strip trademark glyphs and edition suffixes that hurt matching.
pub fn normalize_name(name: &str) -> String {
    let mut s: String = name.chars().filter(|c| !matches!(c, '™' | '®' | '©')).collect();
    for suffix in [
        " - definitive edition",
        ": definitive edition",
        " definitive edition",
        " - game of the year edition",
        " game of the year edition",
        " goty edition",
        " goty",
        " - complete edition",
        ": complete edition",
        " complete edition",
        " - enhanced edition",
        ": enhanced edition",
        " enhanced edition",
        " - special edition",
        ": special edition",
        " special edition",
        " - deluxe edition",
        ": deluxe edition",
        " deluxe edition",
        " - anniversary edition",
        " anniversary edition",
        " - director's cut",
        ": director's cut",
        " director's cut",
        " remastered",
        " - remastered",
        ": remastered",
    ] {
        // Suffixes are ASCII; compare in place (lowercasing can change byte lengths).
        if let Some(cut) = s.len().checked_sub(suffix.len())
            && s.is_char_boundary(cut)
            && s[cut..].eq_ignore_ascii_case(suffix)
        {
            s.truncate(cut);
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether two names that look alike can still be one game: they carry
/// the same sequel numbers (digits, and Roman numerals II to XX read as
/// numbers), and neither only adds plain words to the other ("Hollow" and
/// "Hollow Knight", "DOOM" and "DOOM Eternal") nor runs on into a longer
/// word ("Mirror" and "Mirror's Edge", "Inside" and "Insider"). A subtitle
/// after a colon or a spaced dash, or something in brackets, can be the
/// same game: "The Witcher 3" is "The Witcher 3: Wild Hunt", "Prey" is
/// "Prey (2017)". Nor can names whose differing words are unlike each other:
/// "Outer Wilds" and "Outer Worlds", "Into the Breach" and "Into the Dead".
/// `score` halves a pair that fails it.
fn may_be_same_game(a: &str, b: &str) -> bool {
    let (a, b) = (normalize_name(a).to_lowercase(), normalize_name(b).to_lowercase());
    if a.is_empty() || b.is_empty() {
        return true;
    }
    if sequel_numbers(&a) != sequel_numbers(&b) {
        return false;
    }
    let (short, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    let goes_on = match long.strip_prefix(short.as_str()) {
        Some(rest) if rest.chars().any(char::is_alphanumeric) => {
            rest.starts_with([':', '(', '['])
                || rest.starts_with(char::is_whitespace) && rest.trim_start().starts_with([':', '-', '–', '—', '(', '['])
        }
        _ => true,
    };
    goes_on && differing_words_alike(&name_key(&a), &name_key(&b))
}

/// Whether the words in which two keys differ, once the words they start and
/// end with alike are set aside, could be one name written two ways: a typo
/// or a spelling ("valey" and "valley") can, "wilds" and "worlds" can't. A
/// word added on one side only (a subtitle, "the") is left to the rest of
/// `may_be_same_game` and to the score.
fn differing_words_alike(a: &str, b: &str) -> bool {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split(' ').collect(), b.split(' ').collect());
    let lead = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[lead..].iter().rev().zip(b[lead..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[lead..a.len() - tail], &b[lead..b.len() - tail]);
    a.is_empty() || b.is_empty() || fuzzy(&a.join(" "), &b.join(" ")) >= 0.8
}

/// Edit distance or Jaro-Winkler, whichever is kinder, with Jaro-Winkler's
/// bonus for a shared start trimmed.
fn fuzzy(a: &str, b: &str) -> f64 {
    strsim::normalized_levenshtein(a, b).max(strsim::jaro_winkler(a, b) * 0.92)
}

/// The numbers in a lowercased name that tell a sequel apart: "hades ii" and
/// "hades 2" both give [2]. A year in brackets ("prey (2017)") isn't one.
fn sequel_numbers(name: &str) -> Vec<u32> {
    let mut out: Vec<u32> = name
        .split([' ', ':', ','])
        .filter(|w| !w.starts_with(['(', '[']))
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter_map(|w| match roman(w) {
            Some(n) => Some(n),
            None if !w.is_empty() && w.len() <= 4 && w.bytes().all(|b| b.is_ascii_digit()) => w.parse().ok(),
            None => None,
        })
        .collect();
    out.sort_unstable();
    out
}

/// A lowercased Roman numeral from II to XX as a number.
fn roman(word: &str) -> Option<u32> {
    const ROMAN: [&str; 19] =
        ["ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv", "xv", "xvi", "xvii", "xviii", "xix", "xx"];
    ROMAN.iter().position(|r| *r == word).and_then(|i| u32::try_from(i + 2).ok())
}

/// How alike a looked-up name and a name a source lists are, from 0 to 1.
/// The same name is 1. Names that differ only in edition words, in Roman or
/// Arabic numerals, or in spacing and punctuation are 0.9: "Dishonored -
/// Definitive Edition" and "Dishonored", "Hades II" and "Hades 2", "Baldurs
/// Gate 3" and "Baldur's Gate 3". Anything else gets a fuzzy score, halved
/// when `may_be_same_game` says the two can't be one game, so a sequel or a
/// longer game's name scores below 0.5 and no source takes it. Each source
/// keeps its own threshold.
pub fn score(query: &str, candidate: &str) -> f64 {
    let (a, b) = (name_key(query), name_key(candidate));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    if compact_key(query) == compact_key(candidate) {
        return 0.9;
    }
    let sim = fuzzy(&a, &b);
    if may_be_same_game(query, candidate) { sim } else { sim * 0.5 }
}

/// `loose_key` with the spaces taken out and Roman numerals read as numbers,
/// except as the first word ("V Rising", "X-COM").
fn compact_key(name: &str) -> String {
    loose_key(name)
        .split(' ')
        .enumerate()
        .map(|(i, w)| match roman(w) {
            Some(n) if i > 0 => n.to_string(),
            _ => w.to_string(),
        })
        .collect()
}

/// Pairs of a looked-up name and a name a source lists, and whether they
/// are one game. Every source's matcher runs over them: a pair that is one
/// game scores at least 0.86, the strictest threshold, and a pair that isn't
/// scores below 0.5, so no source takes it.
#[cfg(test)]
pub(crate) const KNOWN_PAIRS: &[(&str, &str, bool)] = &[
    // A sequel, or the game a sequel follows, is another game.
    ("Hollow", "Hollow Knight", false),
    ("Hades II", "Hades", false),
    ("Hades", "Hades II", false),
    ("Hades", "Hades 2", false),
    ("Portal", "Portal 2", false),
    ("Portal 2", "Portal", false),
    ("Half-Life", "Half-Life 2", false),
    ("Spelunky", "Spelunky 2", false),
    ("Slay the Spire", "Slay the Spire II", false),
    ("Far Cry 3", "Far Cry 4", false),
    ("Dark Souls II", "Dark Souls III", false),
    ("Final Fantasy VII", "Final Fantasy VIII", false),
    ("Final Fantasy VIII", "Final Fantasy VII", false),
    ("Final Fantasy VII", "Final Fantasy VII Remake", false),
    ("DOOM", "DOOM Eternal", false),
    ("Celeste", "Celeste Classic", false),
    ("Stardew Valley", "Stardew Valley Expanded", false),
    // A name that runs on into a longer word is another game.
    ("Mirror", "Mirror's Edge", false),
    ("Inside", "Insider", false),
    ("Ghost", "Ghostrunner", false),
    ("Gris", "Grisaia", false),
    ("Spider", "Spider-Man", false),
    // Names that go on in unlike words are other games.
    ("Outer Wilds", "Outer Worlds", false),
    ("The Outer Worlds", "Outer Wilds", false),
    ("Into the Breach", "Into the Dead", false),
    ("Dead Cells", "Dead Space", false),
    ("Ori and the Will of the Wisps", "Ori and the Blind Forest", false),
    ("Fallout: New Vegas", "Fallout Tactics", false),
    ("The Witcher 3: Wild Hunt", "The Witcher 3: Blood and Wine", false),
    ("Celeste", "Celestian Tales", false),
    ("Limbo", "Lumino City", false),
    ("Gris", "Grime", false),
    // The same game under another edition, numeral, spacing or article.
    ("Hollow Knight", "Hollow Knight", true),
    ("Dishonored - Definitive Edition", "Dishonored", true),
    ("Dishonored", "Dishonored - Definitive Edition", true),
    ("The Elder Scrolls V: Skyrim Special Edition", "The Elder Scrolls V: Skyrim", true),
    ("Alan Wake Remastered", "Alan Wake", true),
    ("Mass Effect Legendary Edition", "Mass Effect", true),
    ("Hades II", "Hades 2", true),
    ("Hades 2", "Hades II", true),
    ("Final Fantasy VII", "Final Fantasy 7", true),
    ("Grand Theft Auto V", "Grand Theft Auto 5", true),
    ("Divinity: Original Sin 2 - Definitive Edition", "Divinity: Original Sin II", true),
    ("Baldurs Gate 3", "Baldur's Gate 3", true),
    ("S.T.A.L.K.E.R.: Shadow of Chernobyl", "STALKER: Shadow of Chernobyl", true),
    ("Spider-Man", "Spiderman", true),
    ("Witcher 3: Wild Hunt, The", "The Witcher 3: Wild Hunt", true),
    ("Dark Souls III", "DARK SOULS™ III", true),
    ("Grand Theft Auto: San Andreas", "Grand Theft Auto San Andreas", true),
    ("Metal Gear Solid V: The Phantom Pain", "Metal Gear Solid V: Phantom Pain", true),
    ("Stardew Valey", "Stardew Valley", true),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_pairs_score_on_their_side() {
        for (query, listed, same) in KNOWN_PAIRS {
            let s = score(query, listed);
            assert!(if *same { s >= 0.86 } else { s < 0.5 }, "{query} and {listed} scored {s:.3}");
        }
    }

    #[test]
    fn normalize_handles_non_ascii() {
        assert_eq!(normalize_name("Foo Remastered"), "Foo");
        // "İ" lowercases to two characters; this used to slice mid-character.
        let _ = normalize_name("İİİİİİİİİ Remastered");
        assert_eq!(normalize_name("Ōkami HD"), "Ōkami HD");
    }

    #[test]
    fn trailing_article_matches() {
        assert_eq!(
            name_key("Elder Scrolls IV: Oblivion - Game of the Year Edition Deluxe, The"),
            name_key("The Elder Scrolls IV: Oblivion - Game of the Year Edition Deluxe")
        );
        assert_eq!(loose_key("Witcher 3: Wild Hunt, The"), loose_key("The Witcher 3: Wild Hunt"));
        assert_eq!(name_key("Hat in Time, A"), "a hat in time");
    }
}

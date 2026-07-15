//! The `>` command line: a small verb grammar the DM types.
//!
//! Text entry follows the app's settled pattern (the host captures keystrokes
//! into a draft `String` on `UiState`; the view only displays it), so this
//! module is just the *parser*: it turns a typed line into a [`Command`], and
//! `UiState` dispatches it to machinery that already exists (spawn, the
//! generator overlay, the compendium, the roll log, the campaign clock). Keeping
//! the parse pure and here makes it unit-testable without a running app.

/// One parsed command. The dispatch lives on `UiState`; this is just the shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// `>spawn goblin` — place a statted creature from the bestiary.
    Spawn(String),
    /// `>gen npc` — open the generator overlay on a matching generator.
    Gen(String),
    /// `>find sword` — search the compendium (monsters, items, spells).
    Find(String),
    /// `>roll 2d6+3` — roll to the shared log.
    Roll(String),
    /// `>time 5` — the DM declares time passing (campaign clock).
    Time(u64),
    /// `>` alone, or `>help`.
    Help,
    /// A verb no one knows.
    Unknown(String),
}

/// Parse a command line. A leading `>` is tolerated but not required (the mode
/// is entered by `>`, so the draft usually has none). Verbs are
/// case-insensitive and take a single trailing argument string.
pub fn parse(input: &str) -> Command {
    let line = input.trim().strip_prefix('>').unwrap_or(input.trim()).trim();
    if line.is_empty() {
        return Command::Help;
    }
    let (verb, rest) = match line.split_once(char::is_whitespace) {
        Some((v, r)) => (v, r.trim()),
        None => (line, ""),
    };
    match verb.to_ascii_lowercase().as_str() {
        "spawn" | "s" => Command::Spawn(rest.to_owned()),
        "gen" | "generate" | "g" => Command::Gen(rest.to_owned()),
        "find" | "search" => Command::Find(rest.to_owned()),
        "roll" | "r" => Command::Roll(rest.to_owned()),
        // A bare number is unusable time; that reads as a mistake, not "0".
        "time" | "wait" | "t" => rest.parse::<u64>().map(Command::Time).unwrap_or_else(|_| {
            Command::Unknown(if rest.is_empty() {
                "time (needs a number)".to_owned()
            } else {
                format!("time {rest}")
            })
        }),
        "help" | "?" => Command::Help,
        other => Command::Unknown(other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_parse_with_their_argument() {
        assert_eq!(parse("spawn goblin"), Command::Spawn("goblin".to_owned()));
        assert_eq!(parse("gen npc"), Command::Gen("npc".to_owned()));
        assert_eq!(parse("find rusty sword"), Command::Find("rusty sword".to_owned()));
        assert_eq!(parse("roll 2d6+3"), Command::Roll("2d6+3".to_owned()));
        assert_eq!(parse("time 5"), Command::Time(5));
    }

    #[test]
    fn a_leading_angle_and_case_and_padding_are_tolerated() {
        assert_eq!(parse(">SPAWN Goblin"), Command::Spawn("Goblin".to_owned()));
        assert_eq!(parse("   >  gen   npc  "), Command::Gen("npc".to_owned()));
        // The argument's own case is preserved; only the verb folds.
        assert_eq!(parse("find LongSword"), Command::Find("LongSword".to_owned()));
    }

    #[test]
    fn short_aliases_work() {
        assert_eq!(parse("s goblin"), Command::Spawn("goblin".to_owned()));
        assert_eq!(parse("g npc"), Command::Gen("npc".to_owned()));
        assert_eq!(parse("r 1d20"), Command::Roll("1d20".to_owned()));
        assert_eq!(parse("t 3"), Command::Time(3));
    }

    #[test]
    fn empty_is_help_and_unknown_verbs_are_named() {
        assert_eq!(parse(""), Command::Help);
        assert_eq!(parse(">"), Command::Help);
        assert_eq!(parse("help"), Command::Help);
        assert_eq!(parse("teleport everyone"), Command::Unknown("teleport".to_owned()));
    }

    #[test]
    fn a_non_numeric_time_is_a_mistake_not_zero() {
        // Silently treating "time soon" as 0 would look like it worked.
        assert_eq!(parse("time soon"), Command::Unknown("time soon".to_owned()));
        assert_eq!(parse("time"), Command::Unknown("time (needs a number)".to_owned()));
    }

    #[test]
    fn a_verb_with_no_argument_carries_an_empty_string() {
        assert_eq!(parse("spawn"), Command::Spawn(String::new()));
        assert_eq!(parse("find"), Command::Find(String::new()));
    }
}

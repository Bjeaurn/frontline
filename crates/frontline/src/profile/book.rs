//! The rules of the Profile book (CONTEXT.md: Profile), pure: which names and file ids are
//! allowed, which Profile is active at startup and after one is deleted, and what the first
//! boot does with the files of an install from before there were several Profiles.
//! `profile` does the file and resource work around them.

/// Longest Profile name, in characters (the name shows on every scoreboard).
pub const MAX_NAME_CHARS: usize = 24;
/// Most Profiles an install holds; NEW is refused beyond it.
pub const MAX_PROFILES: usize = 8;
/// The name suggested when nothing better is known.
pub const DEFAULT_NAME: &str = "Player";

const MAX_ID_CHARS: usize = 48;
const MAX_SLUG_CHARS: usize = 32;
/// File names Windows reserves for devices (with any extension): a Profile file can't be one.
const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// A Profile as the book lists it: the file id (`profiles/<id>.ron`) and the display name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Empty,
    /// Another Profile already has it (compared without regard to case).
    Taken,
}

impl NameError {
    pub fn describe(self) -> &'static str {
        match self {
            NameError::Empty => "Type a name",
            NameError::Taken => "Another profile already has that name",
        }
    }
}

/// `raw` as a Profile name: trimmed, control characters dropped, cut to `MAX_NAME_CHARS`
/// characters. `None` when nothing is left.
pub fn clean_name(raw: &str) -> Option<String> {
    let kept: String = raw.chars().filter(|c| !c.is_control()).collect();
    let capped: String = kept.trim().chars().take(MAX_NAME_CHARS).collect();
    let name = capped.trim_end();
    (!name.is_empty()).then(|| name.to_owned())
}

/// The name `raw` makes, if it can be a Profile's name beside `entries`. `renaming` is the id
/// of the Profile being renamed, which may keep its own name (or change its case).
pub fn check_name(
    raw: &str,
    entries: &[Entry],
    renaming: Option<&str>,
) -> Result<String, NameError> {
    let name = clean_name(raw).ok_or(NameError::Empty)?;
    let lower = name.to_lowercase();
    let taken = entries
        .iter()
        .filter(|entry| Some(entry.id.as_str()) != renaming)
        .any(|entry| entry.name.to_lowercase() == lower);
    if taken {
        Err(NameError::Taken)
    } else {
        Ok(name)
    }
}

/// Whether another Profile can be added.
pub fn has_room(entries: &[Entry]) -> bool {
    entries.len() < MAX_PROFILES
}

/// The entry named `name` (without regard to case), for `--name`.
pub fn find_by_name<'a>(entries: &'a [Entry], name: &str) -> Option<&'a Entry> {
    let lower = name.trim().to_lowercase();
    entries
        .iter()
        .find(|entry| entry.name.to_lowercase() == lower)
}

/// The list order: by name without regard to case, then by id.
pub fn order_key(entry: &Entry) -> (String, String) {
    (entry.name.to_lowercase(), entry.id.clone())
}

/// Puts `entries` in list order.
pub fn sort(entries: &mut [Entry]) {
    entries.sort_by_cached_key(order_key);
}

/// Whether `id` can be a file stem of ours: lowercase letters, digits and `-`, no device name.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !RESERVED.contains(&id)
}

/// A file id for a new Profile called `name`: its letters and digits, made unique with `-2`,
/// `-3`... while `taken` says the id is in use.
pub fn new_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug
        .trim_matches('-')
        .chars()
        .take(MAX_SLUG_CHARS)
        .collect();
    let slug = slug.trim_end_matches('-');
    let mut base = if slug.is_empty() {
        "profile".to_owned()
    } else {
        slug.to_owned()
    };
    if RESERVED.contains(&base.as_str()) {
        base = format!("profile-{base}");
    }
    let mut id = base.clone();
    let mut n = 2;
    while taken(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// The Profile active at startup: the one remembered in the Settings if it still exists, else
/// the first in the list. `None` when there is no Profile at all.
pub fn active_at_start(entries: &[Entry], remembered: Option<&str>) -> Option<String> {
    remembered
        .and_then(|id| entries.iter().find(|entry| entry.id == id))
        .or_else(|| entries.first())
        .map(|entry| entry.id.clone())
}

/// The Profile that becomes active when `deleted` goes: the one after it in `entries` (the
/// list before the delete, in list order), else the one before it. `None` when it was the
/// only one.
pub fn active_after_delete(entries: &[Entry], deleted: &str) -> Option<String> {
    let at = entries.iter().position(|entry| entry.id == deleted)?;
    entries
        .get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|before| entries.get(before)))
        .map(|entry| entry.id.clone())
}

/// What the first boot of a build with several Profiles does, from what is on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Startup {
    /// Profiles exist: `active` is the one to use.
    Use { active: String },
    /// An install from before several Profiles: create the first Profile called `name`, with the
    /// data of the old `profile.ron` when `old_file`, and a fresh one when only the old name
    /// in the Settings is left.
    Create { name: String, old_file: bool },
    /// Nothing exists: ask the player for a name.
    AskName,
}

/// Decides the startup. `old_file` is whether the single-Profile `profile.ron` exists,
/// `old_name` the name an old `settings.ron` held (empty when none), `suggestion` the name to
/// fall back to when migrating without one. An old name that is just the default `Player`
/// was never chosen: without an old Profile file it doesn't count, and the player is asked.
pub fn startup(
    entries: &[Entry],
    remembered: Option<&str>,
    old_file: bool,
    old_name: &str,
    suggestion: &str,
) -> Startup {
    if let Some(active) = active_at_start(entries, remembered) {
        return Startup::Use { active };
    }
    match (clean_name(old_name), old_file) {
        (Some(name), true) => Startup::Create {
            name,
            old_file: true,
        },
        (Some(name), false) if name != DEFAULT_NAME => Startup::Create {
            name,
            old_file: false,
        },
        (None, true) => Startup::Create {
            name: clean_name(suggestion).unwrap_or_else(|| DEFAULT_NAME.to_owned()),
            old_file: true,
        },
        (Some(_), false) | (None, false) => Startup::AskName,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str) -> Entry {
        Entry {
            id: id.into(),
            name: name.into(),
        }
    }

    fn book() -> Vec<Entry> {
        vec![entry("ann", "Ann"), entry("bob", "Bob"), entry("cy", "Cy")]
    }

    #[test]
    fn names_are_trimmed_stripped_and_capped() {
        assert_eq!(clean_name("  Mac  "), Some("Mac".into()));
        assert_eq!(clean_name("Ma\tc\n"), Some("Mac".into()));
        assert_eq!(clean_name(" \t "), None);
        assert_eq!(clean_name(""), None);
        let long = "x".repeat(MAX_NAME_CHARS + 10);
        assert_eq!(clean_name(&long).unwrap().chars().count(), MAX_NAME_CHARS);
        // The cut counts characters, not bytes, and doesn't leave trailing blanks.
        let wide = format!("{}  tail", "é".repeat(MAX_NAME_CHARS - 1));
        let cut = clean_name(&wide).unwrap();
        assert_eq!(cut.chars().count(), MAX_NAME_CHARS - 1);
        assert!(!cut.ends_with(' '));
    }

    #[test]
    fn a_name_must_be_non_empty_and_unique_ignoring_case() {
        assert_eq!(check_name("  ", &book(), None), Err(NameError::Empty));
        assert_eq!(check_name("bob", &book(), None), Err(NameError::Taken));
        assert_eq!(check_name(" BOB ", &book(), None), Err(NameError::Taken));
        assert_eq!(check_name("Dee", &book(), None), Ok("Dee".into()));
    }

    #[test]
    fn a_profile_may_keep_its_own_name_when_renamed() {
        assert_eq!(check_name("Bob", &book(), Some("bob")), Ok("Bob".into()));
        assert_eq!(check_name("BOB", &book(), Some("bob")), Ok("BOB".into()));
        assert_eq!(
            check_name("Ann", &book(), Some("bob")),
            Err(NameError::Taken)
        );
    }

    #[test]
    fn the_book_holds_a_few_profiles() {
        assert!(has_room(&book()));
        let full: Vec<Entry> = (0..MAX_PROFILES)
            .map(|n| entry(&format!("p{n}"), &format!("P{n}")))
            .collect();
        assert!(!has_room(&full));
    }

    #[test]
    fn the_list_is_sorted_by_name_ignoring_case() {
        let mut entries = vec![entry("z", "zed"), entry("b", "Bob"), entry("a", "alice")];
        sort(&mut entries);
        let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "z"]);
    }

    #[test]
    fn ids_are_file_safe_and_unique() {
        assert_eq!(new_id("Mac Book!", |_| false), "mac-book");
        assert_eq!(new_id("  --  ", |_| false), "profile");
        assert_eq!(new_id("Äöü", |_| false), "profile");
        let taken = ["mac", "mac-2"];
        assert_eq!(new_id("Mac", |id| taken.contains(&id)), "mac-3");
        let long = new_id(&"a".repeat(100), |_| false);
        assert!(long.len() <= MAX_SLUG_CHARS && is_valid_id(&long));
        // A device name Windows can't use as a file name.
        assert_eq!(new_id("CON", |_| false), "profile-con");
        assert_eq!(new_id("nul", |_| false), "profile-nul");
        let sixty = "x".repeat(60);
        for name in ["Mac", "con", "a b c", "ÄÖÜ", sixty.as_str()] {
            assert!(is_valid_id(&new_id(name, |_| false)), "{name}");
        }
    }

    #[test]
    fn only_our_own_ids_are_accepted_from_disk() {
        assert!(is_valid_id("mac-2"));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("Mac"));
        assert!(!is_valid_id("../mac"));
        assert!(!is_valid_id("a.b"));
        assert!(!is_valid_id("com1"));
        assert!(!is_valid_id(&"a".repeat(MAX_ID_CHARS + 1)));
    }

    #[test]
    fn startup_uses_the_remembered_profile_else_the_first() {
        assert_eq!(active_at_start(&book(), Some("bob")), Some("bob".into()));
        assert_eq!(active_at_start(&book(), Some("gone")), Some("ann".into()));
        assert_eq!(active_at_start(&book(), None), Some("ann".into()));
        assert_eq!(active_at_start(&[], Some("bob")), None);
    }

    #[test]
    fn deleting_moves_to_the_next_profile_then_the_previous() {
        assert_eq!(active_after_delete(&book(), "ann"), Some("bob".into()));
        assert_eq!(active_after_delete(&book(), "bob"), Some("cy".into()));
        assert_eq!(active_after_delete(&book(), "cy"), Some("bob".into()));
        assert_eq!(active_after_delete(&[entry("ann", "Ann")], "ann"), None);
        assert_eq!(active_after_delete(&book(), "nobody"), None);
    }

    #[test]
    fn an_old_install_becomes_the_first_profile_named_from_its_old_name() {
        assert_eq!(
            startup(&[], None, true, "  Mac ", "root"),
            Startup::Create {
                name: "Mac".into(),
                old_file: true
            }
        );
        // Only the old Settings name: a fresh Profile with it, no prompt.
        assert_eq!(
            startup(&[], None, false, "Mac", "root"),
            Startup::Create {
                name: "Mac".into(),
                old_file: false
            }
        );
        // An old Profile file but no usable name falls back to the suggestion, then to Player.
        assert_eq!(
            startup(&[], None, true, "", "root"),
            Startup::Create {
                name: "root".into(),
                old_file: true
            }
        );
        assert_eq!(
            startup(&[], None, true, " ", ""),
            Startup::Create {
                name: DEFAULT_NAME.into(),
                old_file: true
            }
        );
    }

    #[test]
    fn a_fresh_install_asks_and_an_existing_book_is_never_migrated_again() {
        assert_eq!(startup(&[], None, false, "", "root"), Startup::AskName);
        // The default name was never chosen: asked, unless there is an old Profile to keep.
        assert_eq!(
            startup(&[], None, false, "Player", "root"),
            Startup::AskName
        );
        assert_eq!(
            startup(&[], None, true, "Player", "root"),
            Startup::Create {
                name: "Player".into(),
                old_file: true
            }
        );
        // The old file is only read while there are no Profiles; later it is left alone.
        assert_eq!(
            startup(&book(), Some("cy"), true, "Mac", "root"),
            Startup::Use {
                active: "cy".into()
            }
        );
    }
}

//! Text chat and the Level report (server): validates and routes each client's `SendChat`,
//! stores reported Levels as `PlayerLevel`, and gives other server systems `SystemChat` to
//! announce joins, leaves, team switches and level-ups to everyone.

use std::collections::VecDeque;
use std::time::Duration;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use super::LinkPlayer;
use super::protocol::{ChatKind, ChatLine, EventChannel, ReportLevel, SendChat};
use crate::data::GameData;
use crate::shared::{PlayerLevel, PlayerName, PlayerTeam};

/// Longest chat message, in characters, after trimming.
pub const MAX_CHAT_CHARS: usize = 120;
/// A player may send this many messages within `RATE_WINDOW`.
const RATE_MESSAGES: usize = 3;
const RATE_WINDOW: Duration = Duration::from_secs(3);

/// The message a client meant to send: trimmed, non-empty, at most `MAX_CHAT_CHARS` characters
/// and free of control characters. Anything else is dropped.
pub fn clean_chat(text: &str) -> Option<String> {
    let text = text.trim();
    let valid = !text.is_empty()
        && text.chars().count() <= MAX_CHAT_CHARS
        && !text.chars().any(char::is_control);
    valid.then(|| text.to_owned())
}

/// When a player last chatted, to rate-limit them.
#[derive(Component, Default, Debug)]
pub struct ChatRate(VecDeque<Duration>);

impl ChatRate {
    /// Whether a message at `now` is within the limit; counts it when it is.
    pub fn allow(&mut self, now: Duration) -> bool {
        while self
            .0
            .front()
            .is_some_and(|sent| now.saturating_sub(*sent) >= RATE_WINDOW)
        {
            self.0.pop_front();
        }
        if self.0.len() >= RATE_MESSAGES {
            return false;
        }
        self.0.push_back(now);
        true
    }
}

type ChatSenders<'w, 's> = Query<
    'w,
    's,
    (&'static LinkPlayer, &'static mut MessageSender<ChatLine>),
    (With<ClientOf>, With<Connected>),
>;

/// Sends system lines (no sender name) to every client.
#[derive(SystemParam)]
pub struct SystemChat<'w, 's> {
    senders: ChatSenders<'w, 's>,
}

impl SystemChat<'_, '_> {
    pub fn say(&mut self, text: impl Into<String>) {
        let line = ChatLine {
            kind: ChatKind::System,
            name: String::new(),
            text: text.into(),
        };
        info!("[chat] {}", line.text);
        for (_, mut sender) in &mut self.senders {
            sender.send::<EventChannel>(line.clone());
        }
    }
}

/// Keeps each client's reported Level (clamped into the valid range) as its `PlayerLevel`; a
/// higher Level than the last one reported is a level-up, announced to everyone.
pub(super) fn receive_levels(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<ReportLevel>)>,
    mut players: Query<(&mut PlayerLevel, &PlayerName)>,
    data: Res<GameData>,
    mut chat: SystemChat,
) {
    for (player, mut receiver) in &mut links {
        for ReportLevel(reported) in receiver.receive() {
            let Ok((mut level, name)) = players.get_mut(player.0) else {
                continue;
            };
            let reported = data.progression.levels.clamp(reported);
            let previous = level.0;
            level.set_if_neq(PlayerLevel(reported));
            info!("\"{}\" is Level {reported}", name.0);
            if previous != 0 && reported > previous && !name.0.is_empty() {
                chat.say(format!("{} reached Level {reported}", name.0));
            }
        }
    }
}

/// Routes each client's chat message to everyone, or to the sender's Team for team chat,
/// unless it is invalid or over the sender's rate limit.
pub(super) fn receive_chat(
    mut links: Query<(&LinkPlayer, &mut MessageReceiver<SendChat>)>,
    mut players: Query<(&PlayerName, &PlayerTeam, &mut ChatRate)>,
    teams: Query<&PlayerTeam>,
    mut senders: ChatSenders,
    time: Res<Time>,
) {
    for (player, mut receiver) in &mut links {
        for SendChat { team, text } in receiver.receive() {
            let Ok((name, sender_team, mut rate)) = players.get_mut(player.0) else {
                continue;
            };
            let Some(text) = clean_chat(&text) else {
                debug!("\"{}\": invalid chat message dropped", name.0);
                continue;
            };
            if name.0.is_empty() || (team && sender_team.0.is_none()) {
                continue;
            }
            if !rate.allow(time.elapsed()) {
                debug!("\"{}\": chat rate limit, message dropped", name.0);
                continue;
            }
            let line = ChatLine {
                kind: if team { ChatKind::Team } else { ChatKind::All },
                name: name.0.clone(),
                text,
            };
            info!(
                "[chat] {} {}: {}",
                if team { "[TEAM]" } else { "[ALL]" },
                line.name,
                line.text
            );
            for (recipient, mut sender) in &mut senders {
                let hears = !team
                    || teams
                        .get(recipient.0)
                        .is_ok_and(|other| other.0 == sender_team.0);
                if hears {
                    sender.send::<EventChannel>(line.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_trimmed() {
        assert_eq!(clean_chat("  gg wp \n").as_deref(), Some("gg wp"));
    }

    #[test]
    fn empty_and_blank_messages_are_dropped() {
        assert_eq!(clean_chat(""), None);
        assert_eq!(clean_chat("   \t "), None);
    }

    #[test]
    fn length_limit_counts_characters_after_trimming() {
        let at_limit = "é".repeat(MAX_CHAT_CHARS);
        assert_eq!(clean_chat(&format!(" {at_limit} ")), Some(at_limit.clone()));
        assert_eq!(clean_chat(&format!("{at_limit}x")), None);
    }

    #[test]
    fn control_characters_are_dropped() {
        assert_eq!(clean_chat("a\nb"), None);
        assert_eq!(clean_chat("a\u{7}"), None);
    }

    #[test]
    fn rate_limit_allows_three_messages_per_three_seconds() {
        let mut rate = ChatRate::default();
        let at = Duration::from_millis;
        assert!(rate.allow(at(0)));
        assert!(rate.allow(at(500)));
        assert!(rate.allow(at(1000)));
        assert!(!rate.allow(at(1500)));
        assert!(!rate.allow(at(2999)));
        // The first message has left the window; a dropped one never counted.
        assert!(rate.allow(at(3000)));
        assert!(!rate.allow(at(3100)));
        assert!(rate.allow(at(3500)));
    }
}

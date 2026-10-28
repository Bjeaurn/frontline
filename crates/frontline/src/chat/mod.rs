//! Text chat (Client and Host): Y opens an all-chat input, U a team-chat input (`Overlay::Chat`,
//! so the cursor is free and gameplay input ignored while typing); Enter sends it, Esc cancels.
//! The chat box bottom-left shows the last few lines, which fade out; while typing it shows
//! the whole history. Lines come from the server (`ChatLine`): chat from other players (team
//! chat only from teammates) and system lines for level-ups, joins, leaves and team switches.

use std::collections::VecDeque;

use bevy::color::Alpha;
use bevy::input_focus::{AutoFocus, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use lightyear::prelude::{Client, Connected};

use crate::flow::{AppState, Overlay};
use crate::match_hud::FRIENDLY_COLOR;
use crate::net::MAX_CHAT_CHARS;
use crate::net::protocol::{ChatKind, ChatLine, SendChat};
use crate::player::input::Bindings;
use crate::shared::{FromServer, LocalPlayerReplica, PlayerTeam, ToServer};
use crate::ui::{self, UiFonts};

/// Lines kept, and shown while typing.
const HISTORY_KEPT: usize = 30;
/// Lines shown while not typing.
const HISTORY_SHOWN: usize = 6;
/// Seconds a line stays up when not typing, the last `FADE_SECS` of them fading out.
const LINE_LIFE_SECS: f64 = 8.0;
const FADE_SECS: f64 = 1.5;
const TEXT_SIZE: f32 = 18.0;
const INPUT_SIZE: f32 = 20.0;
const BOX_WIDTH: f32 = 560.0;
const LINE_BACKGROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.4);
const SYSTEM_COLOR: Color = ui::TEXT_DIM;

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatLog>()
            .init_resource::<ChatTarget>()
            .add_systems(OnEnter(AppState::InGame), spawn_chat_box)
            .add_systems(OnExit(AppState::InGame), clear_log)
            .add_systems(OnEnter(Overlay::Chat), spawn_input)
            .add_systems(OnExit(Overlay::Chat), release_focus)
            .add_systems(Update, collect_lines)
            .add_systems(
                Update,
                (
                    open_input.run_if(in_state(Overlay::None)),
                    drive_input.run_if(in_state(Overlay::Chat)),
                    (rebuild_lines, style_lines)
                        .chain()
                        .run_if(in_state(AppState::InGame)),
                ),
            );
    }
}

/// One line of the chat box, as it is shown.
struct Entry {
    kind: ChatKind,
    text: String,
    /// `Time::elapsed_secs_f64` when it arrived.
    at: f64,
}

/// The last `HISTORY_KEPT` lines; cleared when leaving the game.
#[derive(Resource, Default)]
struct ChatLog(VecDeque<Entry>);

/// Whether the input being typed goes to the Team (U) or everyone (Y).
#[derive(Resource, Default)]
struct ChatTarget {
    team: bool,
}

/// The bottom-left column: the lines and, while typing, the input row.
#[derive(Component)]
struct ChatBox;

/// The column the lines are rebuilt into.
#[derive(Component)]
struct ChatLines;

/// A line in the box, styled every frame from its age.
#[derive(Component)]
struct ChatLineView {
    at: f64,
    /// 0 for the newest line.
    from_newest: usize,
    color: Color,
}

#[derive(Component)]
struct ChatInput;

/// How a line reads: `[ALL] Name: text`, `[TEAM] Name: text` or the system text itself.
fn format_line(line: &ChatLine) -> String {
    match line.kind {
        ChatKind::All => format!("[ALL] {}: {}", line.name, line.text),
        ChatKind::Team => format!("[TEAM] {}: {}", line.name, line.text),
        ChatKind::System => line.text.clone(),
    }
}

fn line_color(kind: ChatKind) -> Color {
    match kind {
        ChatKind::All => ui::TEXT,
        ChatKind::Team => FRIENDLY_COLOR,
        ChatKind::System => SYSTEM_COLOR,
    }
}

/// Opacity of a line `age` seconds old: opaque, then fading out over its last `FADE_SECS`.
fn fade(age: f64) -> f32 {
    ((LINE_LIFE_SECS - age) / FADE_SECS).clamp(0.0, 1.0) as f32
}

fn spawn_chat_box(mut commands: Commands) {
    commands.spawn((
        Name::new("Chat"),
        ChatBox,
        DespawnOnExit(AppState::InGame),
        GlobalZIndex(5),
        Node {
            position_type: PositionType::Absolute,
            left: px(16.0),
            bottom: px(24.0),
            width: px(BOX_WIDTH),
            flex_direction: FlexDirection::Column,
            row_gap: px(6.0),
            ..default()
        },
        children![(
            ChatLines,
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexStart,
                row_gap: px(2.0),
                ..default()
            },
        )],
    ));
}

fn clear_log(mut log: ResMut<ChatLog>) {
    log.0.clear();
}

/// Adds each line the server sends to the log. Always on: the first lines can arrive while
/// the game screen is still being set up.
fn collect_lines(
    mut lines: MessageReader<FromServer<ChatLine>>,
    mut log: ResMut<ChatLog>,
    time: Res<Time>,
) {
    for FromServer(line) in lines.read() {
        let text = format_line(line);
        info!("Chat: {text}");
        if log.0.len() == HISTORY_KEPT {
            log.0.pop_front();
        }
        log.0.push_back(Entry {
            kind: line.kind,
            text,
            at: time.elapsed_secs_f64(),
        });
    }
}

/// Y / U start typing, once connected (team chat also needs a Team).
fn open_input(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    connected: Query<(), (With<Client>, With<Connected>)>,
    team: Query<&PlayerTeam, With<LocalPlayerReplica>>,
    mut target: ResMut<ChatTarget>,
    mut next: ResMut<NextState<Overlay>>,
) {
    if connected.is_empty() {
        return;
    }
    let team_chat = bindings.chat_team.just_pressed(&keys);
    if !team_chat && !bindings.chat_all.just_pressed(&keys) {
        return;
    }
    if team_chat && team.single().is_ok_and(|team| team.0.is_none()) {
        return;
    }
    target.team = team_chat;
    next.set(Overlay::Chat);
}

/// The input row: who it goes to and a focused single-line field.
fn spawn_input(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    target: Res<ChatTarget>,
    chat_box: Single<Entity, With<ChatBox>>,
) {
    let (tag, color) = if target.team {
        ("[TEAM]", FRIENDLY_COLOR)
    } else {
        ("[ALL]", ui::TEXT)
    };
    commands
        .spawn((
            Name::new("ChatInput"),
            DespawnOnExit(Overlay::Chat),
            ChildOf(*chat_box),
            Node {
                align_items: AlignItems::Center,
                column_gap: px(8.0),
                ..default()
            },
        ))
        .with_children(|row| {
            row.spawn(ui::label(&fonts, tag, INPUT_SIZE, color));
            row.spawn((
                ChatInput,
                AutoFocus,
                ui::text_field(&fonts, "", MAX_CHAT_CHARS),
            ))
            .insert((
                Node {
                    flex_grow: 1.0,
                    padding: UiRect::axes(px(8.0), px(3.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                TextFont {
                    font: fonts.regular.clone().into(),
                    font_size: FontSize::Px(INPUT_SIZE),
                    ..default()
                },
            ));
        });
}

fn release_focus(mut focus: ResMut<InputFocus>) {
    focus.clear();
}

/// Enter sends what was typed (if anything) and closes the input; Esc closes it.
fn drive_input(
    keys: Res<ButtonInput<KeyCode>>,
    field: Query<&EditableText, With<ChatInput>>,
    target: Res<ChatTarget>,
    mut messages: MessageWriter<ToServer<SendChat>>,
    mut next: ResMut<NextState<Overlay>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        next.set(Overlay::None);
    } else if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        if let Ok(field) = field.single() {
            let text = field.value().to_string();
            if !text.trim().is_empty() {
                messages.write(ToServer(SendChat {
                    team: target.team,
                    text,
                }));
            }
        }
        next.set(Overlay::None);
    }
}

/// Rebuilds the lines whenever the log changes.
fn rebuild_lines(
    log: Res<ChatLog>,
    fonts: Res<UiFonts>,
    lines: Single<Entity, With<ChatLines>>,
    mut commands: Commands,
) {
    if !log.is_changed() {
        return;
    }
    let newest = log.0.len().saturating_sub(1);
    commands
        .entity(*lines)
        .despawn_related::<Children>()
        .with_children(|column| {
            for (index, entry) in log.0.iter().enumerate() {
                let color = line_color(entry.kind);
                column.spawn((
                    ChatLineView {
                        at: entry.at,
                        from_newest: newest - index,
                        color,
                    },
                    Node {
                        padding: UiRect::axes(px(6.0), px(1.0)),
                        max_width: px(BOX_WIDTH),
                        ..default()
                    },
                    BackgroundColor(LINE_BACKGROUND),
                    ui::label(&fonts, &entry.text, TEXT_SIZE, color),
                ));
            }
        });
}

/// While typing every line shows; otherwise the newest few do, fading out with age.
fn style_lines(
    overlay: Option<Res<State<Overlay>>>,
    time: Res<Time>,
    mut lines: Query<(
        &ChatLineView,
        &mut Node,
        &mut TextColor,
        &mut BackgroundColor,
    )>,
) {
    let typing = overlay.is_some_and(|overlay| *overlay.get() == Overlay::Chat);
    let now = time.elapsed_secs_f64();
    for (view, mut node, mut text, mut background) in &mut lines {
        let opacity = if typing {
            1.0
        } else if view.from_newest < HISTORY_SHOWN {
            fade(now - view.at)
        } else {
            0.0
        };
        let display = if opacity > 0.0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        text.set_if_neq(TextColor(view.color.with_alpha(opacity)));
        background.set_if_neq(BackgroundColor(
            LINE_BACKGROUND.with_alpha(LINE_BACKGROUND.alpha() * opacity),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_as_all_team_or_system() {
        let line = |kind| ChatLine {
            kind,
            name: "Mac".into(),
            text: "push B".into(),
        };
        assert_eq!(format_line(&line(ChatKind::All)), "[ALL] Mac: push B");
        assert_eq!(format_line(&line(ChatKind::Team)), "[TEAM] Mac: push B");
        assert_eq!(format_line(&line(ChatKind::System)), "push B");
    }

    #[test]
    fn lines_fade_out_at_the_end_of_their_life() {
        assert_eq!(fade(0.0), 1.0);
        assert_eq!(fade(LINE_LIFE_SECS - FADE_SECS), 1.0);
        assert!(fade(LINE_LIFE_SECS - FADE_SECS / 2.0) < 1.0);
        assert_eq!(fade(LINE_LIFE_SECS), 0.0);
        assert_eq!(fade(60.0), 0.0);
    }
}

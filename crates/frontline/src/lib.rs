//! Frontline: the game's modules, shared by the game binary (`frontline`: the windowed game and
//! the headless Dedicated Server) and the map viewer (`mapedit`, crates/mapedit).

pub mod assets;
pub mod atmosphere;
pub mod ballistics;
pub mod chat;
pub mod class;
pub mod cli;
pub mod cod4;
pub mod combat;
pub mod data;
pub mod equipment;
pub mod flow;
pub mod gear;
pub mod loadout;
pub mod maps;
pub mod match_hud;
pub mod materials;
pub mod menus;
pub mod modes;
pub mod net;
pub mod particles;
pub mod player;
pub mod profile;
pub mod progression;
pub mod rear_cue;
pub mod rotation;
pub mod rules;
pub mod shared;
pub mod ui;
pub mod video;
pub mod weapon;
pub mod world;

/// Simulation and network tick rate, shared by client and server.
pub const TICK_HZ: f64 = 64.0;

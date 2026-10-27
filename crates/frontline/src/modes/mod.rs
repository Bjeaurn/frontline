//! Game Mode objectives beyond Team Deathmatch: Domination, Headquarters and Search & Destroy.
//!
//! The rules are pure state machines (`capture`, `dom`, `hq`, `sd`) with unit tests; the server
//! drives them from `combat::mode_systems` and publishes a [`ModeView`] on the replicated Match
//! entity, which the HUD and the in-world visuals (`visuals`) read on every process.

mod capture;
mod dom;
mod hq;
pub mod sd;
mod view;
mod visuals;

pub use capture::{Capture, CaptureEvent, in_zone};
pub use dom::DomState;
pub use hq::{HqEvent, HqPhase, HqState};
pub use sd::{Bomb, SdEvent, SdInput, SdPhase, SdState};
pub use view::{ActionView, BombView, FlagView, HqView, ModeView, SdPhaseView, SdView, ZoneView};
pub use visuals::{ModeVisualsPlugin, Spectating, spectated_name};

//! # t3a-engine — Type3arabi prediction engine
//!
//! Arabizi (Latin + digits) → ranked Arabic candidates. Pure Rust, no platform dependencies
//! (AGENTS.md R12). Specification: `docs/03-engine-algorithm.md`.
//!
//! Status: **seed-only mode** is implemented (rules, OOV lattice, phrases, numbers, joiners, display
//! transforms, vowel-derived harakat, in-memory user model, config). The lexicon/LM engine is M3.
//!
//! ```
//! use t3a_engine::{Engine, EngineSettings, InputChar, NoUser, Session};
//! let engine = Engine::builtin();
//! let mut s = Session::new(&engine, EngineSettings::default());
//! for ch in "7abibi".chars() { s.push(InputChar::new(ch), &NoUser); }
//! assert_eq!(s.candidates().items[0].text, "حبيبي");
//! ```

pub mod alphabet;
pub mod arabic;
pub mod charlm;
pub mod config;
pub mod dialect;
pub mod display;
pub mod journal;
pub mod learning_file;
pub mod normalize;
pub mod oov;
pub mod params;
pub mod search;
pub mod seed;
pub mod session;
pub mod store;
pub mod tashkeel;
pub mod user;

pub use config::Config;
pub use dialect::{Dialect, Posterior};
pub use normalize::InputChar;
pub use params::EngineParams;
pub use seed::SeedTables;
pub use session::{
    Candidate, CandidateKind, CandidateList, Commit, CommitHow, Engine, EngineError,
    EngineSettings, Session, Trailing,
};
pub use store::UserStore;
pub use tashkeel::{LetterSlot, SelectMode, TashkeelAction, TashkeelCmd, TashkeelEditor};
pub use user::{MemoryUser, NoUser, UserScorer};

//! Utilitaires temps — implémentation canonique dans `agent_runtime::time`
//! (C-26 : déduplication de la copie ligne à ligne qui existait ici).

pub use agent_runtime::time::{now_iso, now_unix, now_unix_u64};

//! Tour de conversation : assemblage du prompt multi-tour et orchestration
//! d'une requête Gemini vers les notifications ACP.
//!
//! Architecture modulaire :
//! - [`build`]             — construction du prompt (système + historique + fenêtre glissante).
//! - [`content`]           — conversion `ContentBlock` ACP → texte + images.
//! - [`error`]             — messages d'erreur actionnables.
//! - [`follow_up`]         — parsing et normalisation du composant Gemini `<FollowUp>`.
//! - [`notify`]            — notifications ACP (chunks texte, usage tokens).
//! - [`protocol_filter`]   — dernière barrière de présentation pour les enveloppes protocole.
//! - [`stream`]            — consommation du flux Gemini et lifecycle sémantique.
//! - [`title`]              — dérivation automatique du titre de session.
//! - [`turn`]               — orchestration du tour complet.

pub mod build;
pub mod content;
pub mod error;
pub mod follow_up;
pub mod notify;
mod protocol_filter;
pub mod stream;
pub mod title;
pub mod turn;

pub use turn::run_turn;

//! Tour de conversation : assemblage du prompt multi-tour et orchestration
//! d'une requête Gemini vers les notifications ACP.
//!
//! Architecture modulaire :
//! - [`build`]             — construction du prompt (système + historique + fenêtre glissante).
//! - [`content`]           — conversion `ContentBlock` ACP → texte + images.
//! - [`error`]             — messages d'erreur actionnables.
//! - [`follow_up`]         — parsing et normalisation du composant Gemini `<FollowUp>`.
//! - [`interaction`]       — parsing streaming des groupes `<ElicitationsGroup>`.
//! - [`notify`]            — notifications ACP (chunks texte, usage tokens).
//! - [`protocol`]          — vocabulaire partagé des enveloppes Gemini/ACP.
//! - [`protocol_filter`]   — dernière barrière de présentation pour les enveloppes protocole.
//! - [`stream`]             — consommation du flux Gemini et lifecycle sémantique.
//! - [`stream_contract`]   — contrat unifié raw protocol → ACP presentation.
//! - [`tool_stream`]       — détection incrémentale des protocoles d'appel outil.
//! - [`title`]              — dérivation automatique du titre de session.
//! - [`turn`]               — orchestration du tour complet.

pub mod build;
pub mod content;
pub mod error;
pub mod follow_up;
mod interaction;
pub mod notify;
mod protocol;
mod protocol_filter;
pub mod stream;
mod stream_contract;
mod tool_stream;
pub mod title;
pub mod turn;

pub use turn::run_turn;

//! Erreurs typées partagées (refactor M6 — cf. spec §5.4).
//!
//! Les erreurs Gemini sont typées via `thiserror` pour permettre aux clients
//! (notamment `acp::prompt::run_turn`) de `match` sur des cas spécifiques et
//! renvoyer un message ACP actionnable à l'utilisateur (cookies expirés,
//! modèle inconnu, divergence de flux, etc.) au lieu d'un `anyhow` générique.
//!
//! Points de production réels :
//! - `CookiesExpired` / `UpstreamRejected` : détection wire-level dans
//!   `client/stream.rs` (regex `BardErrorInfo [code]`) ;
//! - `UnknownModel` : `core/models.rs::resolve` (modèle demandé ou défaut
//!   de configuration inconnu) ;
//! - `Network` : erreurs reqwest d'envoi/lecture du flux (`client/stream.rs`) ;
//! - `Http` : statut non-2xx de la réponse Gemini ;
//! - `StreamDivergence` : snapshot du candidat incompatible en cours de stream ;
//! - `UploadFailed` : chemin d'upload Scotty (`client/upload.rs`) ;
//! - `SafetyBlocked` : métadonnée typée `blockReason` du flux décodé.

use thiserror::Error;

/// Erreur du backend Gemini web, produite par `client::stream`,
/// `client::upload` et `core::models` (voir la liste des points de
/// production en tête de fichier).
#[derive(Debug, Error)]
pub enum GeminiError {
    /// Cookies expirés ou invalides — `BardErrorInfo [<code>]` dans le corps.
    /// Code 401 = cookies expirés.
    #[error(
        "cookies expired or invalid (BardErrorInfo [{code}]) — re-export vendor/cookie.json"
    )]
    CookiesExpired { code: i64 },

    /// Rejet explicite du backend Gemini ne permettant pas d'être classé comme
    /// une simple expiration de session.
    #[error("Gemini upstream rejected request: BardErrorInfo [{code}]")]
    UpstreamRejected { code: i64 },

    /// Modèle inconnu — la clé n'est pas dans la table `core::models`.
    #[error("unknown model: {0}")]
    UnknownModel(String),

    /// Erreur réseau (timeout, DNS, connexion rompue).
    #[error("network error: {0}")]
    Network(String),

    /// Erreur HTTP (status non-2xx).
    ///
    /// Le body upstream n'est volontairement jamais stocké dans l'erreur :
    /// une erreur `Debug`, un log structuré ou une remontée inter-couches ne
    /// doit pas pouvoir divulguer des tokens, cookies ou contenu arbitraire.
    #[error("HTTP error {status}")]
    Http { status: u16 },

    /// Divergence de flux en cours de streaming — le texte cumulé a change
    /// de prefix après émission, retry impossible.
    #[error("stream diverged while streaming")]
    StreamDivergence,

    /// Upload Scotty échoué (initiation ou finalisation).
    #[error("Scotty upload failed: {0}")]
    UploadFailed(String),

    /// Blocage par la politique de sécurité de Gemini (blockReason, refus
    /// textuel, ou flux vide sans candidat). Le champ contient la raison
    /// lisible pour l'utilisateur.
    #[error("{0}")]
    SafetyBlocked(String),

    /// Erreur non classée — wrap `anyhow::Error` pour compatibilité ascendante.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type GeminiResult<T> = Result<T, GeminiError>;

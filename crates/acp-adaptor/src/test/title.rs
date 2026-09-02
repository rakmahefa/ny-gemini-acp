use super::*;

#[test]
fn derive_title_court_reste_telquel() {
    assert_eq!(derive_title("Bonjour"), "Bonjour");
    assert_eq!(
        derive_title("Refactor la fonction main"),
        "Refactor la fonction main"
    );
}

#[test]
fn derive_title_long_est_tronque_a_la_limite_partagee() {
    let long = "Ceci est un message utilisateur tres long qui depasse largement la limite de titre et doit etre tronque proprement par le sanitize_title du runtime".repeat(3);
    let title = derive_title(&long);
    assert!(title.ends_with('…'));
    assert!(title.chars().count() <= agent_runtime::session::MAX_TITLE_LENGTH + 1);
}

#[test]
fn derive_title_multiligne_prend_premiere_ligne() {
    assert_eq!(
        derive_title("Première ligne\nDeuxième ligne"),
        "Première ligne"
    );
}

#[test]
fn derive_title_vide_renvoie_defaut() {
    assert_eq!(derive_title(""), "Nouvelle session");
    assert_eq!(derive_title("   \n   "), "Nouvelle session");
}

#[test]
fn derive_title_unicode_compte_chars_pas_octets() {
    let title = derive_title(&"🚀".repeat(300));
    assert!(title.chars().count() <= agent_runtime::session::MAX_TITLE_LENGTH + 1);
}

use super::*;

fn test_session() -> Session {
    Session::new(
        "sess_test".into(),
        "/home/dev/projet".into(),
        vec!["/home/dev/lib".into()],
        "test-model",
    )
}

#[test]
fn persona_default_is_coding() {
    assert_eq!(Persona::default(), Persona::Coding);
}

#[test]
fn persona_parse_insensitive() {
    assert_eq!(Persona::from_str_lossy("CODING"), Some(Persona::Coding));
    assert_eq!(Persona::from_str_lossy("creative"), Some(Persona::Creative));
    assert_eq!(Persona::from_str_lossy("brief"), Some(Persona::Concise));
    assert_eq!(Persona::from_str_lossy("invalid"), None);
}

#[test]
fn system_prompt_contains_cwd_and_roots() {
    let s = test_session();
    let p = system_prompt(&s, None);
    assert!(p.contains("CWD: /home/dev/projet"));
    assert!(p.contains("Racines additionnelles: /home/dev/lib"));
}

#[test]
fn system_prompt_coding_has_execution_contract() {
    let s = test_session();
    let p = system_prompt(&s, Some(Persona::Coding));
    assert!(p.contains("comportement d'agent logiciel rigoureux"));
    assert!(p.contains("Une intention, une promesse"));
    assert!(p.contains("ne constitue jamais l'exécution"));
    assert!(p.contains("inspecter → décider → modifier/exécuter → vérifier → résumer"));
}

#[test]
fn system_prompt_coding_forbids_fake_execution_claims() {
    let s = test_session();
    let p = system_prompt(&s, Some(Persona::Coding));
    assert!(p.contains(
        "Ne jamais prétendre avoir créé, modifié, supprimé, exécuté, compilé, testé ou vérifié"
    ));
    assert!(p.contains("Une sortie textuelle annonçant une action n'est qu'une intention"));
    assert!(p.contains(
        "ne considère jamais une étape comme terminée sur la seule base de ton propre texte"
    ));
}

#[test]
fn system_prompt_coding_requires_real_mutation_tools() {
    let s = test_session();
    let p = system_prompt(&s, Some(Persona::Coding));
    assert!(p.contains("Pour toute modification réelle, utilise file_write, file_edit ou replace_in_file ; pour une commande réelle, utilise shell_exec."));
    assert!(p.contains("Le workspace est la source de vérité"));
    assert!(p.contains("Avant la réponse finale, vérifie que les changements demandés ont effectivement été réalisés"));
}

#[test]
fn system_prompt_coding_forbids_protocol_markers_in_prose() {
    let s = test_session();
    let p = system_prompt(&s, Some(Persona::Coding));
    assert!(p.contains("[Assistant]:"));
    assert!(p.contains("[Tool result]:"));
    assert!(p.contains("tool_call"));
    assert!(p.contains("function_call"));
    assert!(p.contains("marqueurs de protocole"));
}

#[test]
fn system_prompt_coding_has_markdown() {
    let s = test_session();
    let p = system_prompt(&s, Some(Persona::Coding));
    assert!(p.contains("Réponds en Markdown"));
}

#[test]
fn system_prompt_creative_is_verbose() {
    let s = test_session();
    let p = system_prompt(&s, Some(Persona::Creative));
    assert!(p.contains("analogies"));
    assert!(p.contains("détaillées"));
}

#[test]
fn system_prompt_concise_is_brief() {
    let s = test_session();
    let p = system_prompt(&s, Some(Persona::Concise));
    assert!(p.contains("minimum de texte"));
    assert!(p.contains("Code directement"));
}

#[test]
fn system_prompt_has_constraints() {
    let s = test_session();
    let p = system_prompt(&s, None);
    assert!(p.contains("Ne jamais inventer de fichiers"));
    assert!(p.contains("Utilise les outils (file_read, file_write, file_edit, replace_in_file, shell_exec, search, glob, list_directory)"));
}

#[test]
fn all_returns_three() {
    assert_eq!(Persona::all().len(), 3);
}

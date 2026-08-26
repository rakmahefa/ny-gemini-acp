use super::*;
use crate::core::models;
use payload::decode_freq;

#[test]
fn payload_102_cases() {
    let resolved = models::Resolved {
        name: "m".into(),
        mode: 1,
        think: 4,
        extra: Some(vec![(31, 2)]),
    };
    let body = payload::payload("bonjour", &resolved, &[], Some("tok"));
    assert!(body.contains("f.req="));
    assert!(body.contains("&at=tok"));
    let decoded = decode_freq(&body);
    let outer: serde_json::Value = serde_json::from_str(&decoded).unwrap();
    let inner: serde_json::Value = serde_json::from_str(outer[1].as_str().unwrap()).unwrap();
    let arr = inner.as_array().unwrap();
    assert_eq!(arr.len(), 102);
    assert_eq!(arr[79], 1);
    assert_eq!(arr[17][0][0], 4);
    assert_eq!(arr[59].as_str().unwrap().len(), 36);
    assert_eq!(arr[31], 2);
    assert_eq!(arr[0][0], "bonjour");
    assert!(arr[0][3].is_null());
}

#[test]
fn payload_avec_refs_images() {
    let resolved = models::resolve("gemini-3.6-flash", models::DEFAULT_MODEL).unwrap();
    let refs = vec![
        "/generated/image1".to_string(),
        "/generated/image2".to_string(),
    ];
    let body = payload::payload("décris", &resolved, &refs, None);
    let outer: serde_json::Value = serde_json::from_str(&decode_freq(&body)).unwrap();
    let arr: serde_json::Value = serde_json::from_str(outer[1].as_str().unwrap()).unwrap();
    assert_eq!(
        arr[0][3],
        serde_json::json!([
            [null, null, "/generated/image1"],
            [null, null, "/generated/image2"]
        ])
    );
}

#[test]
fn token_extraction() {
    let body = r#"<script>window.WIZ_global_data = {"SAPISID": "x", "SNlM0e":"AbCdEf123", "qKIAYe":"feeds/abc123", "Ylro7b":"CgcSXYZ"};</script>"#;
    let t = payload::extract_page_tokens(body);
    assert_eq!(t.at.unwrap(), "AbCdEf123");
    assert_eq!(t.push_id.unwrap(), "feeds/abc123");
    assert_eq!(t.pctx.unwrap(), "CgcSXYZ");
    assert!(payload::extract_page_tokens("rien ici").at.is_none());
}

#[test]
fn encodage_form() {
    let params = vec![
        ("a b".to_string(), "x=y".to_string()),
        ("c".to_string(), "é".to_string()),
    ];
    assert_eq!(payload::form_urlencode(&params), "a+b=x%3Dy&c=%C3%A9");
}

#[test]
fn stream_item_preserves_semantics() {
    let text = StreamItem::Text("delta".into());
    let tool = StreamItem::ToolCall {
        id: "c1".into(),
        name: "glob".into(),
        arguments: serde_json::json!({"pattern":"*.rs"}),
    };
    assert!(matches!(text, StreamItem::Text(value) if value == "delta"));
    assert!(matches!(tool, StreamItem::ToolCall { id, name, .. } if id == "c1" && name == "glob"));
}

//! `std.json` parse + emit roundtrips.

use mty_stdlib::json::{encode, encode_pretty, parse, Json};
use std::collections::BTreeMap;

#[test]
fn primitives_roundtrip() {
    for input in ["null", "true", "false", "42", "3.14", "\"hello\""] {
        let v = parse(input).expect(input);
        let s = encode(&v).expect("encode");
        let v2 = parse(&s).expect("re-parse");
        assert_eq!(v, v2, "input={input} → {s}");
    }
}

#[test]
fn array_roundtrip() {
    let s = r#"[1,2,3,"x",null,true]"#;
    let v = parse(s).unwrap();
    let again = encode(&v).unwrap();
    assert_eq!(parse(&again).unwrap(), v);
}

#[test]
fn object_roundtrip() {
    let s = r#"{"a":1,"b":[2,3],"c":{"nested":true}}"#;
    let v = parse(s).unwrap();
    let again = encode(&v).unwrap();
    assert_eq!(parse(&again).unwrap(), v);
}

#[test]
fn nested_deep_roundtrip() {
    let s = r#"[{"k":[{"v":[1,2,[3,4,{"end":true}]]}]}]"#;
    let v = parse(s).unwrap();
    let again = encode(&v).unwrap();
    assert_eq!(parse(&again).unwrap(), v);
}

#[test]
fn pretty_is_parseable() {
    let mut m = BTreeMap::new();
    m.insert("a".into(), Json::Num(1.0));
    m.insert("b".into(), Json::Arr(vec![Json::Bool(true)]));
    let pretty = encode_pretty(&Json::Obj(m)).unwrap();
    // Pretty output has newlines and 2-space indent.
    assert!(pretty.contains('\n'));
    assert_eq!(
        parse(&pretty).unwrap(),
        parse(&encode(&parse(&pretty).unwrap()).unwrap()).unwrap()
    );
}

#[test]
fn parse_errors_on_garbage() {
    assert!(parse("{").is_err());
    assert!(parse("[1,2").is_err());
    assert!(parse("not json").is_err());
}

#[test]
fn object_key_order_is_sorted() {
    // BTreeMap sorts keys, so encoded output should put them in lex
    // order regardless of source order.
    let v = parse(r#"{"z":1,"a":2,"m":3}"#).unwrap();
    let s = encode(&v).unwrap();
    let z = s.find("\"z\"").unwrap();
    let a = s.find("\"a\"").unwrap();
    let m = s.find("\"m\"").unwrap();
    assert!(a < m && m < z, "expected sorted keys, got {s}");
}

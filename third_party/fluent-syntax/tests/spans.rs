#![cfg(feature = "spans")]

use fluent_syntax::ast::{Entry, Expression, PatternElement, VariantKey};
use fluent_syntax::parser::parse;

#[test]
fn variant_key_spans_slice_only_the_key_contents() {
    let source = "msg = { $count ->\n    [0] Zero\n    [one] One\n   *[other] Other\n}\n";
    let resource = parse(source).expect("parse select expression fixture");
    let Entry::Message(message) = &resource.body[0] else {
        panic!("expected message entry");
    };
    let Some(pattern) = &message.value else {
        panic!("expected message value");
    };
    let PatternElement::Placeable { expression, .. } = &pattern.elements[0] else {
        panic!("expected select placeable");
    };
    let Expression::Select { variants, .. } = expression else {
        panic!("expected select expression");
    };

    assert_eq!(variants.len(), 3);

    match &variants[0].key {
        VariantKey::NumberLiteral { span, .. } => assert_eq!(&source[span.start..span.end], "0"),
        key => panic!("expected numeric variant key, got {key:?}"),
    }
    match &variants[1].key {
        VariantKey::Identifier { span, .. } => assert_eq!(&source[span.start..span.end], "one"),
        key => panic!("expected identifier variant key, got {key:?}"),
    }
    match &variants[2].key {
        VariantKey::Identifier { span, .. } => {
            assert_eq!(&source[span.start..span.end], "other")
        }
        key => panic!("expected identifier variant key, got {key:?}"),
    }
}

#[test]
fn variant_spans_cover_the_entire_branch_including_default_marker() {
    let source = "msg = { $count ->\n    [0] Zero\n    [one] One\n   *[other] Other\n}\n";
    let resource = parse(source).expect("parse select expression fixture");
    let Entry::Message(message) = &resource.body[0] else {
        panic!("expected message entry");
    };
    let Some(pattern) = &message.value else {
        panic!("expected message value");
    };
    let PatternElement::Placeable { expression, .. } = &pattern.elements[0] else {
        panic!("expected select placeable");
    };
    let Expression::Select { variants, .. } = expression else {
        panic!("expected select expression");
    };

    assert_eq!(&source[variants[0].span.start..variants[0].span.end], "[0] Zero\n");
    assert_eq!(
        &source[variants[1].span.start..variants[1].span.end],
        "[one] One\n"
    );
    assert_eq!(
        &source[variants[2].span.start..variants[2].span.end],
        "*[other] Other\n"
    );
}

mod common;

/// When two UiEntry values share the same module_path, both exported names must be
/// importable from a single `import { Foo, Bar } from '@ui/test-multi'` statement.
#[test]
fn both_exports_from_shared_module_path_are_available() {
    let source = r#"
import { FooWidget, BarWidget } from '@ui/test-multi';
export default function render() {
    return <container>
        <FooWidget />
        <BarWidget />
    </container>;
}
"#;
    let result = common::eval_jsx(source);
    let children = result.layout["children"]
        .as_array()
        .expect("expected children array");
    assert_eq!(children.len(), 2, "expected two child nodes");
    assert_eq!(
        children[0]["tw"], "foo-widget",
        "first child should be FooWidget"
    );
    assert_eq!(
        children[1]["tw"], "bar-widget",
        "second child should be BarWidget"
    );
}

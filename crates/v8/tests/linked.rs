#[test]
fn verified_archive_creates_and_disposes_an_isolate() {
  let platform = v8::new_default_platform(4, false).make_shared();
  v8::V8::initialize_platform(platform);
  v8::V8::initialize();
  assert_eq!(v8::V8::get_version(), "15.8.62.58-rusty");
  {
    let isolate = &mut v8::Isolate::new(Default::default());
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let source = v8::String::new(scope, "21 * 2").unwrap();
    let value = v8::Script::compile(scope, source, None)
      .unwrap()
      .run(scope)
      .unwrap();
    assert_eq!(value.integer_value(scope), Some(42));
  }
  unsafe { v8::V8::dispose() };
  v8::V8::dispose_platform();
}

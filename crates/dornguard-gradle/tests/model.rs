use std::path::Path;

use dornguard_gradle::{CompileModules, collect};

#[tokio::test]
#[ignore = "requires a JDK 21 and the Gradle distribution"]
async fn discovers_projects_and_compiles_selected_java_modules() {
    let root = dunce::canonicalize(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/spring-boot-multimodule"),
    )
    .unwrap();

    let discovery = collect(&root, CompileModules::Selected(&[])).await.unwrap();
    assert_eq!(discovery.modules.len(), 5);
    let api = discovery
        .modules
        .iter()
        .find(|module| module.path == ":api")
        .unwrap();
    assert_eq!(api.project_dir, root.join("api"));
    assert_eq!(api.source_dirs, [root.join("api/src/main/java")]);
    assert_eq!(api.test_source_dirs, [root.join("api/src/test/java")]);
    assert_eq!(api.dependencies, [":common"]);
    assert!(api.classpath.is_empty());
    assert!(api.compile_sources.is_empty());

    let compiled = collect(&root, CompileModules::Selected(&[":api".into()]))
        .await
        .unwrap();
    let api = compiled
        .modules
        .iter()
        .find(|module| module.path == ":api")
        .unwrap();
    assert!(
        api.class_dirs
            .iter()
            .any(|directory| directory.join("example/api/domain/Book.class").is_file())
    );
    assert!(!api.classpath.is_empty());
    assert!(
        api.compile_sources
            .contains(&root.join("api/src/main/java/example/api/domain/Book.java"))
    );
    let admin = compiled
        .modules
        .iter()
        .find(|module| module.path == ":admin")
        .unwrap();
    assert!(admin.classpath.is_empty());
    assert!(admin.compile_sources.is_empty());
}

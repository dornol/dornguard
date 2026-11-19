import org.gradle.api.file.DuplicatesStrategy
import org.gradle.api.tasks.bundling.Jar

plugins {
    java
}

repositories {
    mavenCentral()
}

java {
    toolchain.languageVersion.set(JavaLanguageVersion.of(21))
}

dependencies {
    implementation("com.tngtech.archunit:archunit:1.5.1")
    implementation("com.google.code.gson:gson:2.13.2")
}

tasks.jar {
    archiveFileName.set("archunit-runner.jar")
    manifest.attributes["Main-Class"] = "io.dornguard.runner.Main"
    duplicatesStrategy = DuplicatesStrategy.EXCLUDE
    isPreserveFileTimestamps = false
    isReproducibleFileOrder = true
    from(configurations.runtimeClasspath.get().map { zipTree(it) })
    exclude("META-INF/*.SF", "META-INF/*.RSA", "META-INF/*.DSA", "**/module-info.class")
}

val mavenModel = sourceSets.create("mavenModel")

dependencies {
    add(mavenModel.compileOnlyConfigurationName, "org.apache.maven:maven-core:3.9.9")
    add(mavenModel.implementationConfigurationName, "com.google.code.gson:gson:2.13.2")
}

tasks.named<JavaCompile>(mavenModel.compileJavaTaskName) {
    options.release.set(8)
    options.compilerArgs.add("-Xlint:-options")
}

val mavenModelJar = tasks.register<Jar>("mavenModelJar") {
    archiveFileName.set("maven-model.jar")
    duplicatesStrategy = DuplicatesStrategy.EXCLUDE
    isPreserveFileTimestamps = false
    isReproducibleFileOrder = true
    from(mavenModel.output)
    from(configurations[mavenModel.runtimeClasspathConfigurationName].map { zipTree(it) })
    exclude("META-INF/*.SF", "META-INF/*.RSA", "META-INF/*.DSA", "**/module-info.class")
}

tasks.jar {
    dependsOn(mavenModelJar)
}

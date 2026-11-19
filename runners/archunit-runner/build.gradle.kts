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

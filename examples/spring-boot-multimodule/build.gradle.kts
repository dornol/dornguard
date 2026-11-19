subprojects {
    apply(plugin = "java-library")

    extensions.configure<JavaPluginExtension> {
        toolchain.languageVersion.set(JavaLanguageVersion.of(21))
    }
}

project(":api") {
    dependencies {
        "implementation"(project(":common"))
    }
}

project(":admin") {
    dependencies {
        "implementation"(project(":common"))
    }
}

configure(listOf(project(":admin"), project(":batch"))) {
    repositories.mavenCentral()
    dependencies {
        "implementation"(platform("org.springframework.boot:spring-boot-dependencies:3.5.16"))
    }
}

project(":admin") {
    dependencies {
        "implementation"("org.springframework:spring-web")
    }
}

project(":batch") {
    dependencies {
        "implementation"("org.springframework:spring-context")
    }
}

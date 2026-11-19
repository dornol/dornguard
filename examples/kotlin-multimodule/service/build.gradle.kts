plugins {
    kotlin("jvm")
}

kotlin {
    jvmToolchain(21)
    sourceSets.main {
        kotlin.srcDirs("src/custom/kotlin", "src/shared")
    }
}

sourceSets.main { java.srcDir("src/shared") }

dependencies {
    implementation(project(":library"))
}

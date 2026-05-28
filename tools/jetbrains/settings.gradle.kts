// Mighty JetBrains plugin — single-module gradle build.
//
// We pin the IntelliJ Platform Gradle Plugin 2.x repositories at the settings
// level so that the plugin block in build.gradle.kts can resolve without
// extra configuration.

pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "0.8.0"
}

// Artifact name. The IntelliJ Platform Gradle Plugin uses `rootProject.name`
// for `build/distributions/<name>-<version>.zip`. The mandate wants
// `mighty-0.31.0.zip`.
rootProject.name = "mighty"

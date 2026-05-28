// Mighty JetBrains IDE plugin — Gradle build script.
//
// Powered by the IntelliJ Platform Gradle Plugin 2.x, which is the modern
// successor to the legacy `org.jetbrains.intellij` plugin and the only way to
// reliably target the IntelliJ Platform 2024.x / 2025.x line.
//
// Build:        ./gradlew buildPlugin
// Artifact:     build/distributions/mighty-<version>.zip
// Run sandbox:  ./gradlew runIde   (downloads ~500MB IDE — see README)
// Verify:       ./gradlew verifyPlugin

import org.jetbrains.intellij.platform.gradle.TestFrameworkType

plugins {
    id("java")
    kotlin("jvm") version "1.9.24"
    id("org.jetbrains.intellij.platform") version "2.1.0"
}

group = providers.gradleProperty("pluginGroup").get()
version = providers.gradleProperty("pluginVersion").get()

repositories {
    mavenCentral()
    // The IntelliJ Platform plugin defines its own repository helper that
    // resolves the JetBrains marketplace and IDE distribution servers.
    intellijPlatform {
        defaultRepositories()
    }
}

// JDK 17 is the minimum for IntelliJ Platform 2024.x. JDK 21 also works.
java {
    toolchain {
        languageVersion.set(JavaLanguageVersion.of(17))
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    intellijPlatform {
        // Target IntelliJ IDEA Community as the build base. The plugin
        // remains compatible with every IDE listed in plugin.xml.
        create(
            providers.gradleProperty("platformType").get(),
            providers.gradleProperty("platformVersion").get(),
        )

        // No extra IDE bundled plugins required — LSP API is in platform core.

        // Plugin Verifier — runs `verifyPlugin` against multiple IDE versions.
        pluginVerifier()
        zipSigner()
        // NOTE: `instrumentationTools()` is intentionally omitted. On JDK 21
        // (which we use for the gradle daemon here) the dependency resolver
        // for `intellij.java-compiler-ant-tasks` looks under
        // `${JAVA_HOME}/Packages` — a directory that only exists on the
        // JetBrains Runtime distribution. Bytecode instrumentation just adds
        // optional `@NotNull` runtime checks; skipping it keeps the build
        // green and the produced plugin functions identically.

        testFramework(TestFrameworkType.Platform)
    }
}

intellijPlatform {
    pluginConfiguration {
        version = providers.gradleProperty("pluginVersion")

        // The IntelliJ Platform plugin rewrites plugin.xml's <idea-version>
        // tag with these values so we don't have to maintain them in two
        // places.
        ideaVersion {
            sinceBuild = providers.gradleProperty("pluginSinceBuild")
            untilBuild = providers.provider { null } // forwards-compatible
        }
    }

    pluginVerification {
        ides {
            recommended()
        }
    }
}

tasks {
    wrapper {
        gradleVersion = "8.9"
    }

    // Don't fail the build just because we're forwards-compatible (no
    // until-build means the verifier warns about future IDEs we haven't
    // tested).
    runIde {
        // Allocate more heap when developers actually launch the sandbox IDE.
        jvmArgs = listOf("-Xmx2g")
    }

    // Skip bytecode instrumentation. It would add @NotNull runtime checks
    // via the IntelliJ ant tasks, but the helper looks under
    // `${JAVA_HOME}/Packages` which only exists on the JetBrains Runtime
    // distribution. On regular JDK 21 the dependency resolution fails. The
    // produced plugin works fine without these extra runtime asserts.
    instrumentCode {
        enabled = false
    }
    // `instrumentedJar` is downstream of `instrumentCode`. The IntelliJ
    // Platform Gradle Plugin's `composedJar` task expects the instrumented
    // jar to exist on disk even when we've skipped instrumentation, so we
    // rewire `instrumentedJar` to just package the same classes as the
    // ordinary `jar` task instead of disabling it outright.
    instrumentedJar {
        from(sourceSets.main.get().output)
    }

    // Bundled test task uses the IntelliJ Platform test framework.
    test {
        useJUnitPlatform()
    }
}

import org.gradle.api.tasks.testing.logging.TestLogEvent

plugins {
    id("idea")
    application

    alias(libs.plugins.kotlin)
    alias(libs.plugins.kotlin.serialization)
}

dependencies {
    implementation(libs.bundles.implementation)
    runtimeOnly(libs.slf4j.simple)

    testImplementation(kotlin("test"))
    testImplementation(libs.ktor.server.test.host)
}

kotlin {
    jvmToolchain {
        languageVersion = JavaLanguageVersion.of(25)
        @Suppress("UnstableApiUsage")
        vendor = JvmVendorSpec.JETBRAINS
    }
    compilerOptions {
        optIn.addAll("kotlin.uuid.ExperimentalUuidApi")
        freeCompilerArgs.addAll(
            "-Xexplicit-backing-fields",
            "-Xcontext-parameters"
        )
    }
}

application {
    mainClass = "io.github.valine3gdev.mcguildlink.app.AppKt"
    applicationDefaultJvmArgs = listOf(
        "-Dfile.encoding=UTF-8",
        "-Dstdout.encoding=UTF-8",
        "-Dstderr.encoding=UTF-8"
    )
}

tasks {
    named<JavaExec>("run") {
        jvmArgs(
            "-XX:+AllowEnhancedClassRedefinition",
            "-Dfile.encoding=UTF-8",
            "-Dstdout.encoding=UTF-8",
            "-Dstderr.encoding=UTF-8",
            "-Dorg.slf4j.simpleLogger.defaultLogLevel=debug"
        )
    }
    withType<Test>().configureEach {
        useJUnitPlatform()

        testLogging {
            events(
                TestLogEvent.FAILED,
                TestLogEvent.PASSED,
                TestLogEvent.SKIPPED
            )
        }
    }
}

idea {
    module {
        isDownloadJavadoc = true
        isDownloadSources = true
    }
}

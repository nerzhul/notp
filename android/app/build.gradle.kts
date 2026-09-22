import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations
import java.security.MessageDigest
import javax.inject.Inject

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
}

android {
    namespace = "com.nerzhul.notp"
    compileSdk = 34
    ndkVersion = "26.1.10909125"

    defaultConfig {
        applicationId = "com.nerzhul.notp"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    signingConfigs {
        create("release") {
            val keystorePath = System.getenv("NOTP_KEYSTORE_PATH")
            val keystorePassword = System.getenv("NOTP_KEYSTORE_PASSWORD")
            val keyAlias = System.getenv("NOTP_KEY_ALIAS")
            val keyPassword = System.getenv("NOTP_KEY_PASSWORD")
            if (keystorePath != null) {
                storeFile = file(keystorePath)
                storePassword = keystorePassword
                this.keyAlias = keyAlias
                keyPassword = keyPassword
            }
        }
    }

    buildTypes {
        getByName("debug") {
            isMinifyEnabled = false
        }
        getByName("release") {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = if (System.getenv("NOTP_KEYSTORE_PATH") != null) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
    }

    composeOptions {
        kotlinCompilerExtensionVersion = "1.5.14"
    }

    packaging {
        resources {
            excludes += setOf(
                "/META-INF/{AL2.0,LGPL2.1}",
                "/META-INF/DEPENDENCIES",
                "/META-INF/LICENSE",
                "/META-INF/LICENSE.txt",
                "/META-INF/NOTICE",
                "/META-INF/NOTICE.txt"
            )
        }
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.appcompat)
    implementation(libs.material)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.androidx.security.crypto)
    implementation(libs.androidx.biometric)
    implementation(libs.camerax.core)
    implementation(libs.camerax.camera2)
    implementation(libs.camerax.lifecycle)
    implementation(libs.camerax.view)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons.extended)
    implementation(libs.kotlinx.coroutines.android)

    testImplementation(libs.junit)
}

// Build the Rust UniFFI library via cargo-ndk. Cached via a content hash so a
// Gradle invocation with no Rust source change does not pay the full rebuild.
val cargoNdkBuildDir: org.gradle.api.file.Directory =
    layout.buildDirectory.dir("rust").get()

val rustSrcFiles: FileCollection = files(
    "../Cargo.lock",
    "../Cargo.toml",
    "buildSrc/rust",
    fileTree("../android/rust").exclude("target"),
    fileTree("../src").exclude("bin", "ui.rs")
)

val cargoNdkHash: String = providers
    .provider {
        val md = MessageDigest.getInstance("SHA-256")
        rustSrcFiles.forEach { file ->
            if (file.isFile) {
                md.update(file.name.toByteArray())
                md.update(file.readBytes())
            } else {
                md.update(file.absolutePath.toByteArray())
            }
        }
        md.digest().joinToString("") { "%02x".format(it) }
    }
    .get()

val cargoNdkHashFile: org.gradle.api.file.RegularFile =
    cargoNdkBuildDir.file(".cargo-ndk-buildid")

val cargoNdkStale: Provider<Boolean> =
    provider { !cargoNdkHashFile.get().asFile.exists() || cargoNdkHashFile.get().asFile.readText().trim() != cargoNdkHash }

abstract class CargoNdkTask @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun run() {
        val output = outputDir.get().asFile
        output.parentFile.mkdirs()
        val abis = listOf("arm64-v8a", "armeabi-v7a", "x86_64", "x86")
        abis.forEach { abi ->
            exec.exec {
                workingDir = file("../..")
                commandLine(
                    "cargo", "ndk",
                    "--target", abi,
                    "--platform", "android-26",
                    "-o", output.absolutePath,
                    "build",
                    "-p", "notp-android",
                    "--release",
                    "--locked"
                )
            }
        }
        // Concatenate all per-ABI .so outputs into jniLibs so the Android
        // packaging step picks them up under the right ABI directories.
        abis.forEach { abi ->
            val src = file("$output/$abi/libnotp_android.so")
            val dst = file("src/main/jniLibs/$abi/libnotp_android.so")
            dst.parentFile.mkdirs()
            src.copyTo(dst, overwrite = true)
        }
        cargoNdkHashFile.get().asFile.writeText(cargoNdkHash)
    }
}

val buildRust = tasks.register<CargoNdkTask>("buildRust") {
    group = "rust"
    description = "Compile the notp UniFFI library for all Android ABIs via cargo-ndk"
    outputDir.set(cargoNdkBuildDir.dir("jniLibs"))
}

val generateScaffolding = tasks.register("generateScaffolding") {
    group = "rust"
    description = "Generate Kotlin UniFFI scaffolding for the notp-android crate"
    dependsOn(buildRust)
    doLast {
        val generatedDir = file("src/main/kotlin/com/nerzhul/notp/uniffi")
        generatedDir.mkdirs()
        exec {
            workingDir = file("../..")
            commandLine(
                "uniffi-bindgen",
                "generate",
                "--library",
                "${cargoNdkBuildDir.get().asFile.absolutePath}/arm64-v8a/libnotp_android.so",
                "--language", "kotlin",
                "--out-dir", generatedDir.parentFile.absolutePath,
                "--no-format"
            )
        }
    }
}

tasks.named("preBuild") {
    dependsOn(generateScaffolding)
}
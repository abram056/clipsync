plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

val repoRoot = rootProject.projectDir.parentFile
val generatedBindingsDir = layout.buildDirectory.dir("generated/uniffi")
val cargoJniLibsDir = layout.buildDirectory.dir("jniLibs/cargo")

android {
    namespace = "com.abram056.clipsync"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.abram056.clipsync"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
    }

    sourceSets {
        getByName("main") {
            java.directories.add(generatedBindingsDir.get().asFile.absolutePath)
            kotlin.directories.add(generatedBindingsDir.get().asFile.absolutePath)
            jniLibs.directories.add(cargoJniLibsDir.get().asFile.absolutePath)
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.core:core-ktx:1.19.1")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.11.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.navigation:navigation-compose:2.10.2")
    implementation("net.java.dev.jna:jna:5.19.1@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")

    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test:core:1.7.0")
}

val generateBindings by tasks.registering(Exec::class) {
    group = "bridge"
    description = "Generates Kotlin UniFFI bindings from clipsync.udl"
    workingDir(repoRoot)
    commandLine(
        "cargo", "run", "-q", "-p", "clipsync-bindgen", "--",
        "generate", "crates/clipboard-core/src/clipsync.udl",
        "--language", "kotlin",
        "--out-dir", generatedBindingsDir.get().asFile.absolutePath,
        "--no-format"
    )
    inputs.file(repoRoot.resolve("crates/clipboard-core/src/clipsync.udl"))
    inputs.dir(repoRoot.resolve("crates/clipsync-bindgen/src"))
    outputs.dir(generatedBindingsDir)
}

val buildRustBridge by tasks.registering(Exec::class) {
    group = "bridge"
    description = "Builds the clipboard-core cdylib for Android arm64-v8a"
    workingDir(repoRoot)
    environment(
        "ANDROID_HOME",
        System.getenv("ANDROID_HOME") ?: "${System.getProperty("user.home")}/Android/Sdk"
    )
    commandLine(
        "cargo", "ndk", "-t", "arm64-v8a", "-o", cargoJniLibsDir.get().asFile.absolutePath,
        "build", "-p", "clipboard-core", "--release", "--features", "uniffi"
    )
}

tasks.named("preBuild") {
    dependsOn(generateBindings, buildRustBridge)
}

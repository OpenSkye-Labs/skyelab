// The APK around the Rust cdylib, which ../scripts/build-android.sh puts in ../target/jniLibs/<debug|release>/ first.
plugins {
    id("com.android.application")
}

val mobile = rootDir.parentFile
val gpuiMobile = mobile.resolve("target/gpui-mobile")
val buildDir = mobile.resolve("target/android-host/app")
layout.buildDirectory.set(buildDir)

// gpui-mobile's Rust side binds to dev.gpui.mobile.GpuiInputActivity by JNI name, so we build that one class from
// the checkout, and ship its Apache-2.0 license in the APK's assets.
// A plain File: AGP's source-set DSL refuses a Provider ("You cannot add Provider instances").
val gpuiGenerated = buildDir.resolve("generated/gpui-mobile")
val copyGpuiMobile = tasks.register<Copy>("copyGpuiMobile") {
    from(gpuiMobile.resolve("example/android/gradle/app/src/main/java")) {
        include("dev/gpui/mobile/GpuiInputActivity.java")
        into("java")
    }
    from(gpuiMobile.resolve("LICENSE-APACHE")) {
        rename { "gpui-mobile-LICENSE-APACHE.txt" }
        into("assets/licenses")
    }
    into(gpuiGenerated)
}
tasks.named("preBuild") {
    dependsOn(copyGpuiMobile)
    // Here, not on the copy: a Copy task with no source files is skipped, doFirst included.
    doFirst { check(gpuiMobile.resolve(".ported").exists()) { "run mobile/scripts/fetch-gpui-mobile.sh first" } }
}

// Release signing reads only the environment, never a committed file. With none set, the APK is unsigned (CI).
// SKYELAB_ANDROID_KEYSTORE must be an absolute path.
val signingEnv = listOf(
    "SKYELAB_ANDROID_KEYSTORE",
    "SKYELAB_ANDROID_KEYSTORE_PASSWORD",
    "SKYELAB_ANDROID_KEY_ALIAS",
    "SKYELAB_ANDROID_KEY_PASSWORD",
).associateWith { System.getenv(it).orEmpty() }
val signRelease = signingEnv.values.all { it.isNotEmpty() }
check(signRelease || signingEnv.values.all { it.isEmpty() }) {
    "set all of ${signingEnv.keys} to sign the release APK, or none of them"
}

android {
    namespace = "dev.openskye.skyelab"
    compileSdk = 37

    defaultConfig {
        applicationId = "dev.openskye.skyelab"
        // The cdylib links against this API level (cargo ndk -P 26), gpui-mobile's floor.
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    signingConfigs {
        if (signRelease) {
            create("release") {
                storeFile = file(signingEnv.getValue("SKYELAB_ANDROID_KEYSTORE"))
                storePassword = signingEnv.getValue("SKYELAB_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = signingEnv.getValue("SKYELAB_ANDROID_KEY_ALIAS")
                keyPassword = signingEnv.getValue("SKYELAB_ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false // two small classes; R8 would save nothing next to the 15 MB .so
            if (signRelease) signingConfig = signingConfigs.getByName("release")
        }
    }

    sourceSets {
        getByName("main") {
            java.srcDir(gpuiGenerated.resolve("java"))
            assets.srcDir(gpuiGenerated.resolve("assets"))
        }
        getByName("debug") { jniLibs.srcDir(mobile.resolve("target/jniLibs/debug")) }
        getByName("release") { jniLibs.srcDir(mobile.resolve("target/jniLibs/release")) }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        // Gradle never strips our .so: cargo strips release builds, and stripping would need the NDK on Gradle's side.
        jniLibs {
            keepDebugSymbols += listOf("**/libskyelab_mobile_android.so")
            // cargo-ndk also copies gpui-mobile's own cdylib; ours links gpui-mobile statically and never loads it.
            excludes += listOf("**/libgpui_mobile.so")
        }
    }
}

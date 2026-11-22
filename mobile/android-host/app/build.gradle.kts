// The APK around the Rust cdylib. cargo-ndk writes the .so into ../target/jniLibs/<debug|release>/<abi>/
// first (../scripts/build-android.sh). Outputs go to ../target/android-host so the source tree stays clean.
plugins {
    id("com.android.application")
}

val mobile = rootDir.parentFile
// gpui-mobile, fetched and patched by ../scripts/fetch-gpui-mobile.sh. Its code stays out of our git.
val gpuiMobile = mobile.resolve("target/gpui-mobile")
val buildDir = mobile.resolve("target/android-host/app")
layout.buildDirectory.set(buildDir)

// gpui-mobile's Rust side binds to dev.gpui.mobile.GpuiInputActivity by JNI name, so we build that one class from
// the checkout, and ship its Apache-2.0 license in the APK's assets.
// Plain Files, not Providers: AGP's source-set DSL refuses Providers. preBuild runs the copy first.
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
    doFirst {
        check(gpuiMobile.resolve(".ported").exists()) { "run mobile/scripts/fetch-gpui-mobile.sh first" }
    }
}
tasks.named("preBuild") { dependsOn(copyGpuiMobile) }

// Release signing reads only the environment, never a committed file. All four or none: with none the release
// APK comes out unsigned (app-release-unsigned.apk), which is what CI builds.
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
        // The cdylib links against API 26 (cargo ndk -P 26), gpui-mobile's floor; older devices lack the symbols.
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
        // Only arm64 is built. Every 64-bit Android phone and the Apple-silicon emulator run it.
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
            isMinifyEnabled = false // one small class; nothing worth shrinking
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
        // cargo already strips release builds; this also spares Gradle from needing the NDK to strip.
        jniLibs {
            keepDebugSymbols += listOf("**/libskyelab_mobile_android.so")
            // cargo-ndk also copies gpui-mobile's own cdylib; ours links gpui-mobile statically and never loads it.
            excludes += listOf("**/libgpui_mobile.so")
        }
    }
}

// SkyeLab's Android host: one NativeActivity that loads libskyelab_mobile_android.so (built by cargo-ndk).
// Build it with ../scripts/build-android.sh, which runs cargo first. See docs/mobile.md, step 6.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "SkyeLab"
include(":app")

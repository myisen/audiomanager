// AudioSync Android 应用模块
// T101–T106: JNI 绑定、SAF 适配、前台服务、界面、媒体控制、打包

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.audiosync"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.audiosync"
        minSdk = 26          // Android 8.0：前台服务与 SAF 完整支持
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            // 与 scripts/build-android.sh 中的 Rust target 对应
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    sourceSets {
        getByName("main") {
            // Rust 交叉编译产物位置
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
    packaging {
        resources.excludes += "META-INF/*"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.recyclerview:recyclerview:1.3.2")
    implementation("com.google.android.material:material:1.12.0")
    // T105: 媒体通知与耳机按键（MediaSessionCompat）
    implementation("androidx.media:media:1.7.0")
    // T103: 前台服务生命周期
    implementation("androidx.lifecycle:lifecycle-service:2.8.4")
}

plugins {
    alias(libs.plugins.android.application)
}

// 激活器 App：负责配对 ADB / 推送 agent / 拉起 agent。
// 真正的虚拟屏逻辑在 :agent 模块（独立 jar，跑在 shell 进程里）。
//
// 当前零依赖。做 UI 时再按需加 androidx / material。
android {
    namespace = "com.xiaowine.wcreen"
    compileSdk {
        version = release(37)
    }

    defaultConfig {
        applicationId = "com.xiaowine.wcreen"
        // Android 14 起虚拟屏权限模型才定型（TRUSTED flag 生效），
        // 且与 :agent 模块保持一致，避免两套下限。
        minSdk = 34
        targetSdk = 37
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        release {
            optimization {
                enable = false
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }
}

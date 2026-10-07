plugins {
    alias(libs.plugins.android.application)
}

// 这不是一个可安装的 App。
// 产物是 APK（内含 classes.dex），改名成 .jar 后由 app_process 加载执行。
// 见 agent/src/main/AndroidManifest.xml 与 docs/spec/agent/module.md §5。
android {
    namespace = "com.xiaowine.wcreen.agent"
    compileSdk {
        version = release(37)
    }

    defaultConfig {
        applicationId = "com.xiaowine.wcreen.agent"
        // 目标 Android 14+：TRUSTED / OWN_FOCUS / DEVICE_DISPLAY_GROUP 等 flag 需要 API 33+，
        // 且 Android 14 起虚拟屏权限模型才是现在这个样子。
        minSdk = 34
        targetSdk = 37
        versionCode = 1
        versionName = "1.0"

        // 必须单 dex：app_process 只加载 classes.dex，多出来的 classes2.dex 会被静默忽略，
        // 表现为运行期随机 ClassNotFoundException。显式写死，不依赖 minSdk 的默认行为。
        multiDexEnabled = false
    }

    buildTypes {
        release {
            // 开 R8 裁剪但用 keepRules 里的 -dontobfuscate 保住类名：
            // 裁掉 kotlin-stdlib 中未使用的 ~1000 个类，同时不破坏 app_process 的字符串寻址。
            optimization {
                enable = true
            }
        }
        debug {
            optimization {
                enable = false
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }

    lint {
        // 大量隐藏 API 与高版本 API 调用，lint 的 API 检查没有意义
        disable += "UseRequiresApi"
        abortOnError = false
    }
}

dependencies {
    // 刻意零依赖，唯一例外是 Gson（协议序列化，见 docs/spec/agent/module.md §4）。
    // androidx 需要 Application context 初始化，而 agent 跑在 app_process 的 shell 进程里，
    // 没有真正的 Application。引入 androidx 会在运行时炸掉。
    //
    // kotlin-stdlib 由 AGP 9 内置 Kotlin 支持自动引入，是 Kotlin 代码的运行时必需品
    // （Intrinsics / Unit / 集合类），不能剔除。
    implementation("com.google.code.gson:gson:2.13.2")

    // 只覆盖纯逻辑（协议编解码 / CLI 解析 / flag 位运算 / 状态机），不进产物。
    // 用 JUnit 而不是 kotlin-test：kotlin-test 要写死 Kotlin 版本，
    // 而本仓库的 Kotlin 版本跟随 AGP 内置版本，不单独声明。
    testImplementation("junit:junit:4.13.2")
}

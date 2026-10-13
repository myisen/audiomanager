# AudioSync Android ProGuard 规则
# JNI 函数按名称注册，不能被混淆

# Rust JNI 入口
-keep class com.audiosync.JniBridge { *; }

# 保留 JNI 动态注册所需符号
-keepclasseswithmembernames class * {
    native <methods>;
}

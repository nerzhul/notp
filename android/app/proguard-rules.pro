# Keep UniFFI-generated symbols accessible to the Kotlin scaffolding.
-keep class uniffi.notp_android.** { *; }
-keep class com.nerzhul.notp.uniffi.** { *; }

# Compose
-dontwarn androidx.compose.**
package com.nerzhul.notp.data

import com.nerzhul.notp.uniffi.notp_android.VaultEnvelope
import com.nerzhul.notp.uniffi.notp_android.defaultVaultPath
import com.nerzhul.notp.uniffi.notp_android.peekVaultEnvelope
import java.io.File

/**
 * Canonical on-device path for the user's vault file. Defaults to
 * `dirs::data_dir()` (the app-private files directory on Android).
 */
object VaultPaths {
    fun default(): String = defaultVaultPath()
    fun envelope(path: String): VaultEnvelope = peekVaultEnvelope(path)
    fun exists(path: String): Boolean = File(path).isFile
}
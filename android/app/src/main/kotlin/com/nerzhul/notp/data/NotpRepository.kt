package com.nerzhul.notp.data

import android.content.Context
import androidx.biometric.BiometricManager
import com.nerzhul.notp.crypto.BiometricVault
import com.nerzhul.notp.uniffi.notp_android.AccountDto
import com.nerzhul.notp.uniffi.notp_android.AlgorithmDto
import com.nerzhul.notp.uniffi.notp_android.AppSettingsDto
import com.nerzhul.notp.uniffi.notp_android.NotpVault
import com.nerzhul.notp.uniffi.notp_android.OtpParamsDto
import com.nerzhul.notp.uniffi.notp_android.VaultEnvelope
import com.nerzhul.notp.uniffi.notp_android.VaultError
import com.nerzhul.notp.uniffi.notp_android.Vault_otherError
import com.nerzhul.notp.uniffi.notp_android.vaultCreateWithKey
import com.nerzhul.notp.uniffi.notp_android.vaultUnlockWithKey
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import javax.crypto.Cipher

sealed interface VaultState {
    data object Empty : VaultState
    data class Locked(val path: String) : VaultState
    data class Unlocked(val path: String, val vault: NotpVault) : VaultState
    data class NeedsRecovery(val path: String, val reason: String) : VaultState
}

class NotpRepository(
    private val context: Context,
    private val biometricVault: BiometricVault,
    val settingsStore: AppSettingsStore,
) {

    private val _currentVault = MutableStateFlow<VaultState>(VaultState.Empty)
    val currentVault: StateFlow<VaultState> = _currentVault.asStateFlow()

    val biometricVaultRef: BiometricVault = biometricVault

    @Volatile
    private var lastInteraction: Long = System.currentTimeMillis()
    private var idleJob: Job? = null

    fun refresh() {
        val path = VaultPaths.default()
        biometricVault.loadState()
        val envelope = runCatching { VaultPaths.envelope(path) }.getOrNull()
        _currentVault.value = when {
            !VaultPaths.exists(path) -> VaultState.Empty
            envelope == VaultEnvelope.V1 -> VaultState.NeedsRecovery(
                path = path,
                reason = "Le coffre a été créé sur ordinateur et ne peut pas être ouvert ici.",
            )
            envelope == VaultEnvelope.V3 && biometricVault.currentState() is BiometricVault.State.NeedsRecovery ->
                VaultState.NeedsRecovery(
                    path = path,
                    reason = "L'enrôlement biométrique a été modifié.",
                )
            envelope == VaultEnvelope.V3 -> VaultState.Locked(path)
            else -> VaultState.Empty
        }
    }

    suspend fun createVault(): CreateResult = withContext(Dispatchers.IO) {
        val path = VaultPaths.default()
        val key = biometricVault.provision()
        val vault = try {
            vaultCreateWithKey(path, key.toList())
        } catch (e: VaultError) {
            throw mapError(e)
        }
        _currentVault.value = VaultState.Unlocked(path, vault)
        CreateResult(vault = vault, recoveryKey = com.nerzhul.notp.crypto.RecoveryKey.format(key))
    }

    suspend fun unlockExisting(cipher: Cipher): UnlockResult = withContext(Dispatchers.IO) {
        val path = VaultPaths.default()
        val key = biometricVault.unlock(cipher)
        val vault = try {
            vaultUnlockWithKey(path, key.toList())
        } catch (e: VaultError) {
            throw mapError(e)
        }
        _currentVault.value = VaultState.Unlocked(path, vault)
        UnlockResult(vault = vault)
    }

    suspend fun restoreFromRecovery(recoveryKey: String): RestoreResult = withContext(Dispatchers.IO) {
        val vaultKey = com.nerzhul.notp.crypto.RecoveryKey.parse(recoveryKey)
        val path = VaultPaths.default()
        biometricVault.restore(vaultKey)
        val vault = try {
            vaultUnlockWithKey(path, vaultKey.toList())
        } catch (e: VaultError) {
            throw mapError(e)
        }
        _currentVault.value = VaultState.Unlocked(path, vault)
        RestoreResult(vault = vault)
    }

    fun lockNow() {
        val state = _currentVault.value
        if (state is VaultState.Unlocked) {
            _currentVault.value = VaultState.Locked(state.path)
        }
    }

    fun forgetThisDevice(deleteVaultFile: Boolean) {
        biometricVault.forget()
        if (deleteVaultFile) {
            val path = VaultPaths.default()
            File(path).delete()
            File("$path.bak").delete()
        }
        _currentVault.value = VaultState.Empty
    }

    fun biometricCapability(): BiometricCapability {
        val manager = BiometricManager.from(context)
        return when (manager.canAuthenticate(STRONG_BIOMETRICS)) {
            BiometricManager.BIOMETRIC_SUCCESS -> BiometricCapability.Available
            BiometricManager.BIOMETRIC_ERROR_NO_HARDWARE,
            BiometricManager.BIOMETRIC_ERROR_HW_UNAVAILABLE -> BiometricCapability.Unavailable
            BiometricManager.BIOMETRIC_ERROR_NONE_ENROLLED -> BiometricCapability.NoneEnrolled
            else -> BiometricCapability.Unavailable
        }
    }

    fun recordInteraction() {
        lastInteraction = System.currentTimeMillis()
    }

    suspend fun onForegrounded() {
        recordInteraction()
        val state = _currentVault.value
        if (state !is VaultState.Unlocked) return
        val settings = settingsStore.settings.first()
        val elapsed = (System.currentTimeMillis() - lastInteraction) / 1000
        if (elapsed >= settings.autoLockSeconds) lockNow()
    }

    suspend fun idleWatcher(scope: CoroutineScope) {
        idleJob?.cancel()
        idleJob = scope.launch {
            while (true) {
                delay(1_000)
                val state = _currentVault.value
                if (state is VaultState.Unlocked) {
                    val settings = settingsStore.settings.first()
                    val elapsed = (System.currentTimeMillis() - lastInteraction) / 1000
                    if (elapsed >= settings.autoLockSeconds) {
                        lockNow()
                    }
                }
            }
        }
    }

    private fun mapError(error: VaultError): Throwable {
        val message = (error as? Vault_otherError)?.message ?: error.toString()
        return IllegalStateException(message)
    }

    data class CreateResult(val vault: NotpVault, val recoveryKey: String)
    data class UnlockResult(val vault: NotpVault)
    data class RestoreResult(val vault: NotpVault)

    enum class BiometricCapability { Available, Unavailable, NoneEnrolled }

    companion object {
        private const val STRONG_BIOMETRICS = BiometricManager.Authenticators.BIOMETRIC_STRONG
    }
}

typealias OtpParams = OtpParamsDto
typealias Account = AccountDto
typealias Algorithm = AlgorithmDto
typealias AppSettingsDtoLocal = AppSettingsDto
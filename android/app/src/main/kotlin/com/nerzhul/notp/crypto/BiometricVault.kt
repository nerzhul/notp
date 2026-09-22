package com.nerzhul.notp.crypto

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import androidx.security.crypto.EncryptedFile
import androidx.security.crypto.MasterKey
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/**
 * Wraps / unwraps the 32-byte vault key using a Keystore-resident AES key
 * that requires user authentication. The wrapped key is persisted to an
 * [EncryptedFile] in app-private storage; the unwrapped key only lives in
 * memory for the lifetime of an unlocked session.
 */
class BiometricVault(private val context: Context) {

    sealed interface State {
        data object Uninitialized : State
        data class Wrapped(val wrappedKey: SealedKey) : State
        data class NeedsRecovery(val reason: String) : State
    }

    data class SealedKey(
        val iv: ByteArray,
        val ciphertext: ByteArray
    ) {
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is SealedKey) return false
            return iv.contentEquals(other.iv) && ciphertext.contentEquals(other.ciphertext)
        }
        override fun hashCode(): Int = 31 * iv.contentHashCode() + ciphertext.contentHashCode()
    }

    @Volatile
    private var state: State = State.Uninitialized
    private val keyStore: KeyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

    suspend fun loadState(): State {
        val file = wrappedKeyFile()
        state = if (!file.exists()) {
            State.Uninitialized
        } else {
            try {
                State.Wrapped(readWrappedKey())
            } catch (e: Exception) {
                State.NeedsRecovery(e.message ?: "wrapped key unreadable")
            }
        }
        return state
    }

    fun currentState(): State = state

    /**
     * Provision a fresh vault key: random 32-byte AES key, wrapped by a
     * Keystore key that requires biometric auth. Persists the wrapped key to
     * [wrappedKeyFile]. Idempotent if no vault key is currently stored.
     */
    fun provision(): ByteArray {
        check(state is State.Uninitialized) {
            "Cannot provision while a wrapped key already exists"
        }
        deleteKeystoreEntry()
        val keyGenerator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            ANDROID_KEYSTORE
        )
        val builder = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(true)
            .setInvalidatedByBiometricEnrollment(true)

        try {
            builder.setIsStrongBoxBacked(true)
            keyGenerator.init(builder.build())
            keyGenerator.generateKey()
        } catch (_: Throwable) {
            // Fall back to TEE-backed key when StrongBox is unavailable.
            val fallback = KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setUserAuthenticationRequired(true)
                .setInvalidatedByBiometricEnrollment(true)
            keyGenerator.init(fallback.build())
            keyGenerator.generateKey()
        }

        val vaultKey = ByteArray(32).also { SecureRandom().nextBytes(it) }
        val wrapped = wrapWithKeystore(vaultKey)
        writeWrappedKey(wrapped)
        state = State.Wrapped(wrapped)
        return vaultKey
    }

    /**
     * Unwrap the vault key, prompting the user for biometric authentication
     * via [authCipher]. Returns the raw 32-byte key on success; throws
     * [KeyPermanentlyInvalidatedException] when the wrapping key has been
     * invalidated by a biometric enrollment change.
     */
    fun unlock(authCipher: Cipher): ByteArray {
        val current = state
        if (current !is State.Wrapped) {
            throw IllegalStateException("No wrapped key to unlock")
        }
        val cipher = authCipher
        cipher.init(Cipher.DECRYPT_MODE, loadKeystoreKey())
        cipher.updateAAD(WRAPPED_AAD)
        return try {
            cipher.doFinal(current.wrappedKey.ciphertext)
        } catch (e: KeyPermanentlyInvalidatedException) {
            state = State.NeedsRecovery(e.message ?: "Key invalidated")
            throw e
        }
    }

    /**
     * Re-provision a wrapping Keystore key using the supplied [vaultKey]
     * (typically derived from a recovery key). Persists the new wrapped key.
     */
    fun restore(vaultKey: ByteArray): ByteArray {
        require(vaultKey.size == 32) { "Vault key must be 32 bytes" }
        deleteKeystoreEntry()
        val keyGenerator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            ANDROID_KEYSTORE
        )
        val spec = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(true)
            .setInvalidatedByBiometricEnrollment(true)
            .build()
        keyGenerator.init(spec)
        keyGenerator.generateKey()

        val wrapped = wrapWithKeystore(vaultKey)
        writeWrappedKey(wrapped)
        state = State.Wrapped(wrapped)
        return vaultKey
    }

    /**
     * Forget the wrapping key and the stored wrapped blob. Does NOT delete
     * the encrypted vault file — the user can still recover via the recovery
     * key on the next launch.
     */
    fun forget() {
        deleteKeystoreEntry()
        wrappedKeyFile().delete()
        state = State.Uninitialized
    }

    fun cipherForUnlock(): Cipher {
        val cipher = Cipher.getInstance("${KeyProperties.KEY_ALGORITHM_AES}/${KeyProperties.BLOCK_MODE_GCM}/${KeyProperties.ENCRYPTION_PADDING_NONE}")
        cipher.init(Cipher.DECRYPT_MODE, loadKeystoreKey())
        return cipher
    }

    private fun wrapWithKeystore(vaultKey: ByteArray): SealedKey {
        val cipher = Cipher.getInstance(
            "${KeyProperties.KEY_ALGORITHM_AES}/${KeyProperties.BLOCK_MODE_GCM}/${KeyProperties.ENCRYPTION_PADDING_NONE}"
        )
        cipher.init(Cipher.ENCRYPT_MODE, loadKeystoreKey())
        cipher.updateAAD(WRAPPED_AAD)
        val ciphertext = cipher.doFinal(vaultKey)
        return SealedKey(iv = cipher.iv, ciphertext = ciphertext)
    }

    private fun loadKeystoreKey(): SecretKey {
        val entry = keyStore.getEntry(KEY_ALIAS, null)
            ?: throw IllegalStateException("Keystore entry missing")
        return (entry as KeyStore.SecretKeyEntry).secretKey
    }

    private fun wrappedKeyFile(): File = File(context.filesDir, WRAPPED_FILE_NAME)

    private fun writeWrappedKey(wrapped: SealedKey) {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        val encrypted = EncryptedFile.Builder(
            context,
            wrappedKeyFile(),
            masterKey,
            EncryptedFile.FileEncryptionScheme.AES256_GCM_HKDF_4KB
        ).build()
        encrypted.openFileOutput().use { output ->
            output.write(wrapped.iv.size)
            output.write(wrapped.iv)
            output.write(wrapped.ciphertext.size.toByteArray().padTo4())
            output.write(wrapped.ciphertext)
        }
    }

    private fun readWrappedKey(): SealedKey {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        val encrypted = EncryptedFile.Builder(
            context,
            wrappedKeyFile(),
            masterKey,
            EncryptedFile.FileEncryptionScheme.AES256_GCM_HKDF_4KB
        ).build()
        val bytes = encrypted.openFileInput().use { it.readBytes() }
        var offset = 0
        val ivLen = bytes[offset].toInt()
        offset += 1
        val iv = bytes.copyOfRange(offset, offset + ivLen)
        offset += ivLen
        val ctLen = ByteArray(4).also { buf ->
            System.arraycopy(bytes, offset, buf, 0, 4)
        }.toInt()
        offset += 4
        val ciphertext = bytes.copyOfRange(offset, offset + ctLen)
        return SealedKey(iv = iv, ciphertext = ciphertext)
    }

    private fun deleteKeystoreEntry() {
        if (keyStore.containsAlias(KEY_ALIAS)) {
            keyStore.deleteEntry(KEY_ALIAS)
        }
    }

    private fun ByteArray.padTo4(): ByteArray {
        val padded = ByteArray(4)
        System.arraycopy(this, 0, padded, 0, size.coerceAtMost(4))
        return padded
    }

    companion object {
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        private const val KEY_ALIAS = "biometric_vault_v1"
        private const val WRAPPED_FILE_NAME = "wrapped_key.bin"
        private val WRAPPED_AAD: ByteArray = "notp-v3-wrapped-key".toByteArray()
    }
}
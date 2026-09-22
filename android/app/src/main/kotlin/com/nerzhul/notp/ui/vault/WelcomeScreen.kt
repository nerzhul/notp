package com.nerzhul.notp.ui.vault

import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import androidx.navigation.NavController
import com.nerzhul.notp.R
import com.nerzhul.notp.data.NotpRepository
import com.nerzhul.notp.ui.nav.Routes
import kotlinx.coroutines.launch
import javax.crypto.Cipher

@Composable
fun WelcomeScreen(
    repository: NotpRepository,
    navController: NavController,
) {
    val context = LocalContext.current
    val vaultExists = remember { repository.currentVault.value !is com.nerzhul.notp.data.VaultState.Empty }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    val coroutineScope = androidx.compose.runtime.rememberCoroutineScope()

    Scaffold { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(horizontal = 24.dp, vertical = 32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(24.dp),
        ) {
            Text(
                text = stringResource(R.string.welcome_title),
                style = MaterialTheme.typography.headlineLarge,
            )
            Text(
                text = stringResource(R.string.welcome_subtitle),
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(modifier = Modifier.height(48.dp))
            if (vaultExists) {
                Button(
                    onClick = {
                        unlockWithBiometric(
                            context = context,
                            repository = repository,
                            onUnlocked = {
                                navController.navigate(Routes.OtpList.route) {
                                    popUpTo(Routes.Welcome.route) { inclusive = true }
                                }
                            },
                            onError = { errorMessage = it },
                            onMissing = { errorMessage = "Biométrie indisponible" },
                            coroutineScope = coroutineScope,
                        )
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(stringResource(R.string.welcome_unlock))
                }
            } else {
                Button(
                    onClick = {
                        navController.navigate(Routes.CreateVault.route)
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(stringResource(R.string.welcome_create))
                }
            }
            errorMessage?.let {
                Text(
                    text = it,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
    }
}

internal fun unlockWithBiometric(
    context: android.content.Context,
    repository: NotpRepository,
    onUnlocked: () -> Unit,
    onError: (String) -> Unit,
    onMissing: () -> Unit,
    coroutineScope: kotlinx.coroutines.CoroutineScope,
) {
    val activity = context as? FragmentActivity
        ?: run {
            onError("Activité incompatible avec la biométrie")
            return
        }
    val executor = ContextCompat.getMainExecutor(activity)
    val callback = object : BiometricPrompt.AuthenticationCallback() {
        override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
            val cipher = result.cryptoObject?.cipher ?: return onError("Cipher manquant")
            coroutineScope.launch {
                runCatching { repository.unlockExisting(cipher) }
                    .onSuccess { onUnlocked() }
                    .onFailure { onError(it.message ?: "Échec") }
            }
        }
        override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
            onError(errString.toString())
        }
    }
    val prompt = BiometricPrompt(activity, executor, callback)
    val info = BiometricPrompt.PromptInfo.Builder()
        .setTitle(activity.getString(R.string.biometric_prompt_title))
        .setSubtitle(activity.getString(R.string.biometric_prompt_subtitle))
        .setNegativeButtonText(activity.getString(R.string.biometric_prompt_negative))
        .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG)
        .build()
    try {
        val cipher: Cipher = repository.biometricVaultRef.cipherForUnlock()
        prompt.authenticate(info, BiometricPrompt.CryptoObject(cipher))
    } catch (t: Throwable) {
        onMissing()
    }
}
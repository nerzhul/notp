package com.nerzhul.notp.ui.vault

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import androidx.navigation.NavController
import com.nerzhul.notp.R
import com.nerzhul.notp.data.NotpRepository
import com.nerzhul.notp.ui.nav.Routes
import kotlinx.coroutines.launch

@Composable
fun CreateVaultScreen(
    repository: NotpRepository,
    navController: NavController,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var status by remember { mutableStateOf("") }
    var inFlight by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    Scaffold { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(24.dp),
        ) {
            Text(
                text = stringResource(R.string.create_vault_title),
                style = MaterialTheme.typography.headlineMedium,
            )
            Spacer(modifier = Modifier.height(24.dp))
            if (inFlight) {
                CircularProgressIndicator()
                Text(text = status)
            } else {
                Button(
                    onClick = {
                        val activity = context as? FragmentActivity ?: run {
                            error = "Activité incompatible"
                            return@Button
                        }
                        inFlight = true
                        status = activity.getString(R.string.create_vault_step_biometric)
                        val executor = ContextCompat.getMainExecutor(activity)
                        val callback = object : BiometricPrompt.AuthenticationCallback() {
                            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                                status = activity.getString(R.string.create_vault_step_provisioning)
                                scope.launch {
                                    runCatching { repository.createVault() }
                                        .onSuccess { result ->
                                            inFlight = false
                                            navController.navigate(
                                                Routes.ShowRecoveryKey.build(
                                                    java.net.URLEncoder.encode(
                                                        result.recoveryKey,
                                                        "UTF-8",
                                                    )
                                                )
                                            )
                                        }
                                        .onFailure {
                                            error = it.message
                                            inFlight = false
                                        }
                                }
                            }
                            override fun onAuthenticationError(code: Int, msg: CharSequence) {
                                error = msg.toString()
                                inFlight = false
                            }
                        }
                        val prompt = BiometricPrompt(activity, executor, callback)
                        val info = BiometricPrompt.PromptInfo.Builder()
                            .setTitle(activity.getString(R.string.biometric_prompt_title))
                            .setSubtitle(activity.getString(R.string.biometric_prompt_subtitle))
                            .setNegativeButtonText(activity.getString(R.string.biometric_prompt_negative))
                            .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG)
                            .build()
                        prompt.authenticate(info)
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(stringResource(R.string.create_vault_start))
                }
            }
            error?.let {
                Text(text = it, color = MaterialTheme.colorScheme.error)
            }
        }
    }
}
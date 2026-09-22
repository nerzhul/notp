package com.nerzhul.notp.ui.vault

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.navigation.NavController
import com.nerzhul.notp.R
import com.nerzhul.notp.data.NotpRepository

@Composable
fun RecoveryRequiredScreen(
    repository: NotpRepository,
    onUnlockWithRecovery: () -> Unit,
) {
    val reason = (repository.currentVault.value as? com.nerzhul.notp.data.VaultState.NeedsRecovery)?.reason
        ?: "L'accès biométrique n'est plus valide."

    Scaffold { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                text = stringResource(R.string.recovery_required_title),
                style = MaterialTheme.typography.headlineSmall,
            )
            Text(text = stringResource(R.string.recovery_required_explanation))
            Text(text = reason, color = MaterialTheme.colorScheme.error)
            Spacer(modifier = Modifier.height(16.dp))
            Button(
                onClick = onUnlockWithRecovery,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.recovery_required_submit))
            }
        }
    }
}

@Composable
fun UnlockWithRecoveryScreen(
    repository: NotpRepository,
    navController: NavController,
) {
    var text by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf("") }
    var error by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf<String?>(null) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val inFlight = androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(false) }

    Scaffold { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                text = stringResource(R.string.recovery_required_title),
                style = MaterialTheme.typography.headlineSmall,
            )
            Text(text = stringResource(R.string.recovery_required_explanation))
            androidx.compose.material3.OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                modifier = Modifier.fillMaxWidth(),
                label = { Text("Clé de récupération") },
                isError = error != null,
                singleLine = false,
            )
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            Button(
                onClick = {
                    inFlight.value = true
                    scope.launch {
                        runCatching { repository.restoreFromRecovery(text) }
                            .onSuccess {
                                navController.navigate(com.nerzhul.notp.ui.nav.Routes.OtpList.route) {
                                    popUpTo(0) { inclusive = true }
                                }
                            }
                            .onFailure { error = it.message; inFlight.value = false }
                    }
                },
                enabled = !inFlight.value && text.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.recovery_required_submit))
            }
        }
    }
}
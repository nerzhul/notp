package com.nerzhul.notp.ui.otp

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.navigation.NavController
import com.nerzhul.notp.R
import com.nerzhul.notp.data.NotpRepository
import com.nerzhul.notp.ui.nav.Routes
import com.nerzhul.notp.uniffi.notp_android.AlgorithmDto
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddEditOtpScreen(
    repository: NotpRepository,
    accountId: String?,
    navController: NavController,
) {
    val vault = (repository.currentVault.value as? com.nerzhul.notp.data.VaultState.Unlocked)?.vault
        ?: return
    val scope = rememberCoroutineScope()
    val isNew = accountId == null
    val accounts = remember(vault) { vault.data() }
    val original = accounts.firstOrNull { it.id == accountId }

    var issuer by remember { mutableStateOf(original?.issuer.orEmpty()) }
    var name by remember { mutableStateOf(original?.name.orEmpty()) }
    var secret by remember { mutableStateOf("") }
    var digits by remember { mutableStateOf(original?.digits?.toString() ?: "6") }
    var period by remember { mutableStateOf(original?.period?.toString() ?: "30") }
    var algorithm by remember { mutableStateOf(original?.algorithm ?: AlgorithmDto.Sha1) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(accountId) {
        if (original != null) {
            secret = runCatching { vault.revealSecret(accountId!!) }.getOrDefault("")
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        if (isNew) stringResource(R.string.otp_edit_title_new)
                        else stringResource(R.string.otp_edit_title_edit)
                    )
                },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.Default.ArrowBack, contentDescription = null)
                    }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier.fillMaxSize().padding(padding).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            OutlinedTextField(
                value = issuer, onValueChange = { issuer = it },
                label = { Text(stringResource(R.string.otp_edit_issuer)) },
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = name, onValueChange = { name = it },
                label = { Text(stringResource(R.string.otp_edit_name)) },
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = secret, onValueChange = { secret = it },
                label = { Text(stringResource(R.string.otp_edit_secret)) },
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = digits, onValueChange = { digits = it.filter(Char::isDigit).take(1) },
                label = { Text(stringResource(R.string.otp_edit_digits)) },
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = period, onValueChange = { period = it.filter(Char::isDigit).take(3) },
                label = { Text(stringResource(R.string.otp_edit_period)) },
                modifier = Modifier.fillMaxWidth(),
            )
            AlgorithmChooser(algorithm = algorithm, onChange = { algorithm = it })
            error?.let { Text(it) }
            androidx.compose.material3.Button(
                onClick = {
                    scope.launch {
                        runCatching {
                            val parsedDigits = digits.toIntOrNull() ?: 6
                            val parsedPeriod = period.toIntOrNull() ?: 30
                            if (isNew) {
                                vault.addAccount(
                                    issuer = issuer,
                                    name = name,
                                    secret = secret,
                                    digits = parsedDigits.toUByte().toInt(),
                                    period = parsedPeriod.toUInt().toInt(),
                                    algorithm = algorithm,
                                )
                            } else {
                                vault.replaceAccount(
                                    id = accountId!!,
                                    issuer = issuer,
                                    name = name,
                                    secret = secret,
                                    digits = parsedDigits.toUByte().toInt(),
                                    period = parsedPeriod.toUInt().toInt(),
                                    algorithm = algorithm,
                                )
                            }
                        }.onSuccess {
                            navController.popBackStack()
                        }.onFailure {
                            error = it.message
                        }
                    }
                },
                modifier = Modifier.fillMaxWidth(),
                enabled = issuer.isNotBlank() && name.isNotBlank() && secret.isNotBlank(),
            ) {
                Text(stringResource(R.string.otp_edit_save))
            }
            TextButton(onClick = { navController.popBackStack() }) {
                Text(stringResource(R.string.otp_edit_cancel))
            }
        }
    }
}

@Composable
private fun AlgorithmChooser(algorithm: AlgorithmDto, onChange: (AlgorithmDto) -> Unit) {
    androidx.compose.material3.SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
        AlgorithmDto.entries.forEachIndexed { index, value ->
            androidx.compose.material3.SegmentedButton(
                selected = value == algorithm,
                onClick = { onChange(value) },
                shape = androidx.compose.material3.SegmentedButtonDefaults.itemShape(
                    index = index,
                    count = AlgorithmDto.entries.size,
                ),
            ) { Text(value.name) }
        }
    }
}
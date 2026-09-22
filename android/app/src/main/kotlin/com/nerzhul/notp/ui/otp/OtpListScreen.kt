package com.nerzhul.notp.ui.otp

import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
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
import androidx.core.content.ContextCompat
import androidx.navigation.NavController
import com.nerzhul.notp.R
import com.nerzhul.notp.data.NotpRepository
import com.nerzhul.notp.uniffi.notp_android.AccountDto
import com.nerzhul.notp.ui.nav.Routes
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
fun OtpListScreen(
    repository: NotpRepository,
    navController: NavController,
) {
    val context = LocalContext.current
    val vaultState = repository.currentVault.collectAsStateLifecycleSafe()
    val vault = (vaultState as? com.nerzhul.notp.data.VaultState.Unlocked)?.vault
        ?: return
    val accountsState = remember(vault) { mutableStateOf<List<AccountDto>>(emptyList()) }
    LaunchedEffect(vault) {
        accountsState.value = vault.data()
    }
    var query by remember { mutableStateOf("") }
    var now by remember { mutableStateOf(System.currentTimeMillis() / 1000) }
    LaunchedEffect(Unit) {
        while (true) {
            now = System.currentTimeMillis() / 1000
            delay(1000)
        }
    }
    val filtered by remember(accountsState.value, query) {
        derivedStateOf {
            if (query.isBlank()) accountsState.value
            else accountsState.value.filter {
                it.issuer.contains(query, ignoreCase = true) ||
                    it.name.contains(query, ignoreCase = true)
            }
        }
    }
    val codes = remember(accountsState.value, now) {
        accountsState.value.associate { account ->
            account.id to runCatching { vault.generateCode(account.id, now) }
                .getOrElse { "------" }
        }
    }
    val showMenu = remember { mutableStateOf(false) }
    val confirmForget = remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.app_name)) },
                actions = {
                    IconButton(onClick = { navController.navigate(Routes.Settings.route) }) {
                        Icon(Icons.Default.Settings, contentDescription = null)
                    }
                    IconButton(onClick = { showMenu.value = true }) {
                        Icon(Icons.Default.MoreVert, contentDescription = null)
                    }
                    DropdownMenu(expanded = showMenu.value, onDismissRequest = { showMenu.value = false }) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.otp_list_lock)) },
                            onClick = {
                                showMenu.value = false
                                repository.lockNow()
                            },
                        )
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.otp_list_forget)) },
                            onClick = {
                                showMenu.value = false
                                confirmForget.value = true
                            },
                        )
                    }
                },
            )
        },
        floatingActionButton = {
            androidx.compose.material3.FloatingActionButton(onClick = {
                navController.navigate(Routes.OtpScan.route)
            }) {
                Icon(Icons.Default.Add, contentDescription = null)
            }
        },
    ) { padding ->
        Column(modifier = Modifier.fillMaxSize().padding(padding)) {
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                modifier = Modifier.fillMaxWidth().padding(16.dp),
                label = { Text(stringResource(R.string.otp_list_search_hint)) },
                singleLine = true,
            )
            if (filtered.isEmpty()) {
                Text(
                    text = stringResource(R.string.otp_list_empty),
                    modifier = Modifier.fillMaxWidth().padding(24.dp),
                    style = MaterialTheme.typography.bodyLarge,
                )
            } else {
                LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    items(filtered, key = { it.id }) { account ->
                        AccountRow(
                            account = account,
                            code = codes[account.id] ?: "------",
                            now = now,
                            onClick = {
                                navController.navigate(Routes.OtpDetail.build(account.id))
                            },
                            onCopy = {
                                copyToClipboard(context, codes[account.id] ?: "", label = "otp")
                                scope.launch {
                                    runCatching { vault.recordUse(account.id) }
                                }
                            },
                        )
                    }
                }
            }
        }
    }

    if (confirmForget.value) {
        AlertDialog(
            onDismissRequest = { confirmForget.value = false },
            title = { Text(stringResource(R.string.forget_confirm_title)) },
            text = { Text(stringResource(R.string.forget_confirm_message)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmForget.value = false
                    repository.forgetThisDevice(deleteVaultFile = true)
                    navController.navigate(Routes.Welcome.route) {
                        popUpTo(0) { inclusive = true }
                    }
                }) { Text(stringResource(R.string.forget_confirm_confirm)) }
            },
            dismissButton = {
                TextButton(onClick = { confirmForget.value = false }) {
                    Text(stringResource(R.string.forget_confirm_cancel))
                }
            },
        )
    }
}

@Composable
private fun AccountRow(
    account: AccountDto,
    code: String,
    now: Long,
    onClick: () -> Unit,
    onCopy: () -> Unit,
) {
    val remaining = account.period.toLong() - (now % account.period.toLong())
    val totalSeconds = account.period.toLong()
    androidx.compose.material3.Card(
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(text = account.issuer, style = MaterialTheme.typography.titleMedium)
                Text(text = account.name, style = MaterialTheme.typography.bodyMedium)
                Text(
                    text = code,
                    style = MaterialTheme.typography.headlineSmall,
                    modifier = Modifier.padding(top = 4.dp),
                )
                LinearProgressIndicator(
                    progress = { remaining.toFloat() / totalSeconds.toFloat() },
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                )
            }
            androidx.compose.material3.TextButton(onClick = onCopy) {
                Text("Copier")
            }
        }
    }
}

private fun copyToClipboard(context: android.content.Context, text: String, label: String) {
    val manager = ContextCompat.getSystemService(context, ClipboardManager::class.java)
    manager?.setPrimaryClip(ClipData.newPlainText(label, text))
}

@Composable
private fun <T> kotlinx.coroutines.flow.StateFlow<T>.collectAsStateLifecycleSafe(): T {
    val state = androidx.compose.runtime.collectAsState(initial = value)
    return state.value
}
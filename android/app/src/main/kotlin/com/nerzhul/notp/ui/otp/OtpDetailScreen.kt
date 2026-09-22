package com.nerzhul.notp.ui.otp

import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.navigation.NavController
import com.nerzhul.notp.R
import com.nerzhul.notp.data.NotpRepository
import com.nerzhul.notp.ui.nav.Routes
import kotlinx.coroutines.delay

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun OtpDetailScreen(
    repository: NotpRepository,
    accountId: String,
    navController: NavController,
) {
    val context = LocalContext.current
    val vault = (repository.currentVault.value as? com.nerzhul.notp.data.VaultState.Unlocked)?.vault
        ?: return
    val accounts = remember(vault) { vault.data() }
    val account = accounts.firstOrNull { it.id == accountId }
    var code by remember { mutableStateOf("") }
    var secret by remember { mutableStateOf<String?>(null) }
    var now by remember { mutableStateOf(System.currentTimeMillis() / 1000) }

    LaunchedEffect(accountId) {
        while (true) {
            now = System.currentTimeMillis() / 1000
            code = runCatching { vault.generateCode(accountId, now) }.getOrElse { "------" }
            delay(1000)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(account?.issuer.orEmpty()) },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.Default.ArrowBack, contentDescription = null)
                    }
                },
                actions = {
                    IconButton(onClick = {
                        navController.navigate(Routes.OtpEdit.edit(accountId))
                    }) {
                        Icon(Icons.Default.Edit, contentDescription = null)
                    }
                },
            )
        },
    ) { padding ->
        if (account == null) {
            Box(modifier = Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                Text("Compte introuvable")
            }
            return@Scaffold
        }
        Column(
            modifier = Modifier.fillMaxSize().padding(padding).padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            val remaining = account.period.toLong() - (now % account.period.toLong())
            val progress = remaining.toFloat() / account.period.toFloat()
            Box(contentAlignment = Alignment.Center) {
                CircularProgressIndicator(
                    progress = { progress },
                    modifier = Modifier.size(160.dp),
                    strokeWidth = 12.dp,
                    color = when {
                        remaining <= 5 -> MaterialTheme.colorScheme.error
                        remaining <= 10 -> Color(0xFFE5A100)
                        else -> MaterialTheme.colorScheme.primary
                    },
                )
                Text(
                    text = "${remaining}s",
                    style = MaterialTheme.typography.headlineMedium,
                )
            }
            Text(text = code, style = MaterialTheme.typography.displayLarge)
            androidx.compose.material3.Button(
                onClick = {
                    copyToClipboard(context, code, label = "otp")
                },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.otp_detail_copy))
            }
            androidx.compose.material3.OutlinedButton(
                onClick = {
                    secret = secret ?: runCatching { vault.revealSecret(accountId) }.getOrNull()
                    secret?.let { copyToClipboard(context, it, label = "otp-secret") }
                },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.otp_detail_secret_copy))
            }
            Text(text = "Compte : ${account.name}", style = MaterialTheme.typography.bodyLarge)
            Text(
                text = "Créé le : ${formatTimestamp(account.addedAt)}",
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

private fun formatTimestamp(seconds: Long): String =
    java.text.SimpleDateFormat("yyyy-MM-dd HH:mm", java.util.Locale.getDefault())
        .format(java.util.Date(seconds * 1000))

private fun copyToClipboard(context: android.content.Context, text: String, label: String) {
    val manager = ContextCompat.getSystemService(context, ClipboardManager::class.java)
    manager?.setPrimaryClip(ClipData.newPlainText(label, text))
}
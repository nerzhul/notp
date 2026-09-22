package com.nerzhul.notp.ui.vault

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import com.nerzhul.notp.R
import java.net.URLDecoder

@Composable
fun ShowRecoveryKeyScreen(
    recoveryKey: String,
    onDone: () -> Unit,
) {
    val context = LocalContext.current
    val decoded = remember(recoveryKey) {
        runCatching { URLDecoder.decode(recoveryKey, "UTF-8") }.getOrDefault(recoveryKey)
    }
    var acknowledge by remember { mutableStateOf(false) }
    var understand by remember { mutableStateOf(false) }
    val canFinish = acknowledge && understand

    Scaffold { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(24.dp)
                .verticalScroll(rememberScrollState()),
            horizontalAlignment = Alignment.Start,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                text = stringResource(R.string.recovery_key_title),
                style = MaterialTheme.typography.headlineSmall,
            )
            Text(
                text = stringResource(R.string.recovery_key_intro),
                style = MaterialTheme.typography.bodyMedium,
            )
            Text(
                text = decoded,
                style = MaterialTheme.typography.titleLarge.copy(fontSize = 18.sp),
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 12.dp),
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(checked = acknowledge, onCheckedChange = { acknowledge = it })
                Text(stringResource(R.string.recovery_key_acknowledge))
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Checkbox(checked = understand, onCheckedChange = { understand = it })
                Text(stringResource(R.string.recovery_key_understand))
            }
            Spacer(modifier = Modifier.height(8.dp))
            Row(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.fillMaxWidth(),
            ) {
                OutlinedButton(
                    onClick = {
                        copyToClipboard(context, decoded, label = "notp recovery key")
                    },
                    modifier = Modifier.weight(1f),
                ) {
                    Text(stringResource(R.string.recovery_key_copy))
                }
                OutlinedButton(
                    onClick = { shareText(context, decoded) },
                    modifier = Modifier.weight(1f),
                ) {
                    Text(stringResource(R.string.recovery_key_share))
                }
            }
            Button(
                onClick = onDone,
                enabled = canFinish,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.recovery_key_finish))
            }
        }
    }
}

private fun copyToClipboard(context: Context, text: String, label: String) {
    val manager = ContextCompat.getSystemService(context, ClipboardManager::class.java)
    manager?.setPrimaryClip(ClipData.newPlainText(label, text))
}

private fun shareText(context: Context, text: String) {
    val intent = android.content.Intent(android.content.Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(android.content.Intent.EXTRA_TEXT, text)
    }
    context.startActivity(android.content.Intent.createChooser(intent, null))
}

@Composable
private fun Row(
    horizontalArrangement: Arrangement.Horizontal,
    modifier: Modifier = Modifier,
    verticalAlignment: Alignment.Vertical = Alignment.Top,
    content: @Composable androidx.compose.foundation.layout.RowScope.() -> Unit,
) {
    androidx.compose.foundation.layout.Row(
        horizontalArrangement = horizontalArrangement,
        verticalAlignment = verticalAlignment,
        modifier = modifier,
        content = content,
    )
}
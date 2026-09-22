package com.nerzhul.notp.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
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
import com.nerzhul.notp.data.AppSettings
import com.nerzhul.notp.data.NotpRepository
import com.nerzhul.notp.data.ThemePreference
import com.nerzhul.notp.ui.nav.Routes
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    repository: NotpRepository,
    navController: NavController,
) {
    val settings by repository.settingsStore.settings.collectAsState(
        initial = AppSettings()
    )
    var draft by remember(settings) { mutableStateOf(settings) }
    val scope = rememberCoroutineScope()
    val confirmForget = remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.settings_title)) },
                navigationIcon = {
                    IconButton(onClick = { navController.popBackStack() }) {
                        Icon(Icons.Default.ArrowBack, contentDescription = null)
                    }
                },
            )
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(stringResource(R.string.settings_theme), style = MaterialTheme.typography.titleMedium)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ThemePreference.entries.forEach { theme ->
                    val selected = draft.theme == theme
                    androidx.compose.material3.FilterChip(
                        selected = selected,
                        onClick = {
                            draft = draft.copy(theme = theme)
                            scope.launch { repository.settingsStore.update(draft) }
                        },
                        label = {
                            Text(
                                when (theme) {
                                    ThemePreference.System -> stringResource(R.string.settings_theme_system)
                                    ThemePreference.Light -> stringResource(R.string.settings_theme_light)
                                    ThemePreference.Dark -> stringResource(R.string.settings_theme_dark)
                                },
                            )
                        },
                    )
                }
            }
            Text(
                text = "${stringResource(R.string.settings_autolock)} : ${draft.autoLockSeconds} ${stringResource(R.string.settings_autolock_unit)}",
                style = MaterialTheme.typography.titleMedium,
            )
            Slider(
                value = draft.autoLockSeconds.toFloat(),
                onValueChange = { value ->
                    draft = draft.copy(autoLockSeconds = value.toLong())
                },
                onValueChangeFinished = {
                    scope.launch { repository.settingsStore.update(draft) }
                },
                valueRange = 5f..3600f,
            )
            Text(
                text = "${stringResource(R.string.settings_clipboard)} : ${draft.clipboardClearSeconds} ${stringResource(R.string.settings_clipboard_unit)}",
                style = MaterialTheme.typography.titleMedium,
            )
            Slider(
                value = draft.clipboardClearSeconds.toFloat(),
                onValueChange = { value ->
                    draft = draft.copy(clipboardClearSeconds = value.toLong())
                },
                onValueChangeFinished = {
                    scope.launch { repository.settingsStore.update(draft) }
                },
                valueRange = 0f..600f,
            )
            TextButton(
                onClick = { repository.lockNow() },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.settings_lock_now))
            }
            TextButton(
                onClick = { confirmForget.value = true },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.settings_forget))
            }
            TextButton(
                onClick = { navController.navigate(Routes.About.route) },
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(stringResource(R.string.settings_about))
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
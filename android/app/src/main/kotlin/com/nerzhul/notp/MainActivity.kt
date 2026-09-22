package com.nerzhul.notp

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.nerzhul.notp.data.AppSettings
import kotlinx.coroutines.launch
import com.nerzhul.notp.data.VaultState
import com.nerzhul.notp.ui.nav.NotpNavHost
import com.nerzhul.notp.ui.theme.NotpTheme
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val repository = (application as NotpApplication).repository
        // Block screenshots and the recents-screen preview while the vault
        // holds secrets. The scanner screen opts out locally because the
        // camera preview interacts poorly with FLAG_SECURE on some OEMs.
        window.setFlags(
            WindowManager.LayoutParams.FLAG_SECURE,
            WindowManager.LayoutParams.FLAG_SECURE
        )
        setContent {
            val settings by repository.settingsStore.settings.collectAsState(
                initial = AppSettings()
            )
            val vaultState by repository.currentVault.collectAsState()
            NotpTheme(theme = settings.theme) {
                val route = routeFor(vaultState)
                NotpNavHost(
                    repository = repository,
                    initialRoute = route,
                )
            }
        }
        // Background auto-lock: if the vault is unlocked and the user leaves
        // the app for longer than the configured threshold, drop the in-memory
        // key. Re-entry requires biometric authentication.
        lifecycleScope.launch {
            repeatOnLifecycle(androidx.lifecycle.Lifecycle.State.STARTED) {
                repository.idleWatcher(this@MainActivity)
            }
        }
    }

    override fun onResume() {
        super.onResume()
        val repository = (application as NotpApplication).repository
        lifecycleScope.launch { repository.onForegrounded() }
    }

    private fun routeFor(state: VaultState): String = when (state) {
        VaultState.Empty -> "welcome"
        is VaultState.Locked -> "welcome"
        is VaultState.Unlocked -> "otp_list"
        is VaultState.NeedsRecovery -> "recovery_required"
    }
}
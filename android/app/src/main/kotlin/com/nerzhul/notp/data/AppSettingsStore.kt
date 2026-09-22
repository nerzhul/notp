package com.nerzhul.notp.data

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

enum class ThemePreference { System, Light, Dark }

data class AppSettings(
    val autoLockSeconds: Long = 60,
    val clipboardClearSeconds: Long = 30,
    val theme: ThemePreference = ThemePreference.System,
)

private val Context.settingsDataStore by preferencesDataStore(name = "notp_settings")

class AppSettingsStore(private val context: Context) {

    val settings: Flow<AppSettings> = context.settingsDataStore.data.map { prefs ->
        AppSettings(
            autoLockSeconds = prefs[KEY_AUTO_LOCK] ?: DEFAULT_AUTO_LOCK,
            clipboardClearSeconds = prefs[KEY_CLIPBOARD_CLEAR] ?: DEFAULT_CLIPBOARD_CLEAR,
            theme = prefs[KEY_THEME]?.let {
                runCatching { ThemePreference.valueOf(it) }.getOrNull()
            } ?: ThemePreference.System,
        )
    }

    suspend fun update(settings: AppSettings) {
        context.settingsDataStore.edit { prefs ->
            prefs[KEY_AUTO_LOCK] = settings.autoLockSeconds
            prefs[KEY_CLIPBOARD_CLEAR] = settings.clipboardClearSeconds
            prefs[KEY_THEME] = settings.theme.name
        }
    }

    companion object {
        const val DEFAULT_AUTO_LOCK: Long = 60
        const val DEFAULT_CLIPBOARD_CLEAR: Long = 30
        val KEY_AUTO_LOCK = longPreferencesKey("auto_lock_seconds")
        val KEY_CLIPBOARD_CLEAR = longPreferencesKey("clipboard_clear_seconds")
        val KEY_THEME = stringPreferencesKey("theme")
    }
}
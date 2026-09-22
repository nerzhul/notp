package com.nerzhul.notp

import android.app.Application
import com.nerzhul.notp.crypto.BiometricVault
import com.nerzhul.notp.data.AppSettingsStore
import com.nerzhul.notp.data.NotpRepository

class NotpApplication : Application() {

    val settingsStore: AppSettingsStore by lazy { AppSettingsStore(this) }
    val biometricVault: BiometricVault by lazy { BiometricVault(this) }
    val repository: NotpRepository by lazy {
        NotpRepository(this, biometricVault, settingsStore)
    }

    override fun onCreate() {
        super.onCreate()
        repository.refresh()
    }
}
package com.nerzhul.notp.ui.nav

sealed class Routes(val route: String) {
    data object Welcome : Routes("welcome")
    data object OtpList : Routes("otp_list")
    data object OtpDetail : Routes("otp_detail/{id}") {
        fun build(id: String) = "otp_detail/$id"
    }
    data object OtpEdit : Routes("otp_edit/{id}") {
        fun newEntry() = "otp_edit/new"
        fun edit(id: String) = "otp_edit/$id"
    }
    data object OtpScan : Routes("otp_scan")
    data object Settings : Routes("settings")
    data object About : Routes("about")
    data object CreateVault : Routes("create_vault")
    data object ShowRecoveryKey : Routes("show_recovery/{recovery}") {
        fun build(recovery: String) = "show_recovery/$recovery"
    }
    data object RecoveryRequired : Routes("recovery_required")
    data object UnlockWithRecovery : Routes("unlock_with_recovery")
}
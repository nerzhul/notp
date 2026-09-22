package com.nerzhul.notp.ui.nav

import androidx.compose.runtime.Composable
import androidx.navigation.NavHostController
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import com.nerzhul.notp.data.NotpRepository
import com.nerzhul.notp.ui.otp.AddEditOtpScreen
import com.nerzhul.notp.ui.otp.OtpDetailScreen
import com.nerzhul.notp.ui.otp.OtpListScreen
import com.nerzhul.notp.ui.otp.OtpScanScreen
import com.nerzhul.notp.ui.settings.AboutScreen
import com.nerzhul.notp.ui.settings.SettingsScreen
import com.nerzhul.notp.ui.vault.CreateVaultScreen
import com.nerzhul.notp.ui.vault.RecoveryRequiredScreen
import com.nerzhul.notp.ui.vault.ShowRecoveryKeyScreen
import com.nerzhul.notp.ui.vault.UnlockWithRecoveryScreen
import com.nerzhul.notp.ui.vault.WelcomeScreen

@Composable
fun NotpNavHost(
    repository: NotpRepository,
    initialRoute: String = Routes.Welcome.route,
    navController: NavHostController = rememberNavController(),
) {
    NavHost(navController = navController, startDestination = initialRoute) {
        composable(Routes.Welcome.route) {
            WelcomeScreen(repository = repository, navController = navController)
        }
        composable(Routes.CreateVault.route) {
            CreateVaultScreen(repository = repository, navController = navController)
        }
        composable(
            route = Routes.ShowRecoveryKey.route,
            arguments = listOf(navArgument("recovery") { type = NavType.StringType }),
        ) { backStackEntry ->
            val recovery = backStackEntry.arguments?.getString("recovery").orEmpty()
            ShowRecoveryKeyScreen(
                recoveryKey = recovery,
                onDone = {
                    navController.navigate(Routes.OtpList.route) {
                        popUpTo(Routes.Welcome.route) { inclusive = true }
                    }
                },
            )
        }
        composable(Routes.RecoveryRequired.route) {
            RecoveryRequiredScreen(
                repository = repository,
                onUnlockWithRecovery = { navController.navigate(Routes.UnlockWithRecovery.route) },
            )
        }
        composable(Routes.UnlockWithRecovery.route) {
            UnlockWithRecoveryScreen(
                repository = repository,
                navController = navController,
            )
        }
        composable(Routes.OtpList.route) {
            OtpListScreen(repository = repository, navController = navController)
        }
        composable(
            route = Routes.OtpDetail.route,
            arguments = listOf(navArgument("id") { type = NavType.StringType }),
        ) { backStackEntry ->
            val id = backStackEntry.arguments?.getString("id").orEmpty()
            OtpDetailScreen(repository = repository, accountId = id, navController = navController)
        }
        composable(
            route = Routes.OtpEdit.route,
            arguments = listOf(navArgument("id") { type = NavType.StringType }),
        ) { backStackEntry ->
            val id = backStackEntry.arguments?.getString("id").orEmpty()
            AddEditOtpScreen(
                repository = repository,
                accountId = if (id == "new") null else id,
                navController = navController,
            )
        }
        composable(Routes.OtpScan.route) {
            OtpScanScreen(repository = repository, navController = navController)
        }
        composable(Routes.Settings.route) {
            SettingsScreen(repository = repository, navController = navController)
        }
        composable(Routes.About.route) {
            AboutScreen(repository = repository)
        }
    }
}
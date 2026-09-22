package com.nerzhul.notp.ui.otp

import android.Manifest
import android.content.pm.PackageManager
import android.util.Size
import android.view.WindowManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.navigation.NavController
import com.nerzhul.notp.R
import com.nerzhul.notp.data.NotpRepository
import com.nerzhul.notp.ui.nav.Routes
import com.nerzhul.notp.uniffi.notp_android.OtpParamsDto
import com.nerzhul.notp.uniffi.notp_android.decodeQrFromBytes
import java.util.concurrent.Executors

@Composable
fun OtpScanScreen(
    repository: NotpRepository,
    navController: NavController,
) {
    val context = LocalContext.current
    val activity = context as? androidx.fragment.app.FragmentActivity
    // Scanner screen must NOT carry FLAG_SECURE — it blocks the camera
    // preview on some OEM ROMs. The activity sets the flag globally; clear
    // it locally and restore it on dispose.
    DisposableEffect(Unit) {
        activity?.window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose {
            activity?.window?.setFlags(
                WindowManager.LayoutParams.FLAG_SECURE,
                WindowManager.LayoutParams.FLAG_SECURE,
            )
        }
    }
    var hasPermission by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        )
    }
    val permissionLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestPermission(),
    ) { granted -> hasPermission = granted }

    LaunchedEffect(Unit) {
        if (!hasPermission) {
            permissionLauncher.launch(Manifest.permission.CAMERA)
        }
    }

    val results = remember { mutableStateOf<List<OtpParamsDto>>(emptyList()) }

    Scaffold { padding ->
        Column(modifier = Modifier.fillMaxSize().padding(padding)) {
            Text(
                text = stringResource(R.string.scan_title),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(16.dp),
            )
            if (!hasPermission) {
                Box(modifier = Modifier.fillMaxSize(), contentAlignment = androidx.compose.ui.Alignment.Center) {
                    Text(stringResource(R.string.scan_permission_denied))
                }
                return@Scaffold
            }
            Box(modifier = Modifier.fillMaxSize()) {
                AndroidView(
                    modifier = Modifier.fillMaxSize(),
                    factory = { ctx ->
                        val previewView = PreviewView(ctx)
                        val executor = Executors.newSingleThreadExecutor()
                        val providerFuture = ProcessCameraProvider.getInstance(ctx)
                        providerFuture.addListener({
                            val provider = providerFuture.get()
                            val preview = Preview.Builder().build().also {
                                it.setSurfaceProvider(previewView.surfaceProvider)
                            }
                            val analyzer = ImageAnalysis.Builder()
                                .setResolutionSelector(
                                    ResolutionSelector.Builder()
                                        .setResolutionStrategy(
                                            ResolutionStrategy(
                                                Size(640, 480),
                                                ResolutionStrategy.FALLBACK_RULE_CLOSEST_LOWER_THEN_HIGHER,
                                            ),
                                        ).build(),
                                )
                                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                                .build()
                            analyzer.setAnalyzer(executor) { proxy ->
                                val buffer = proxy.planes[0].buffer
                                val bytes = ByteArray(buffer.remaining()).also { buffer.get(it) }
                                val found = decodeQrFromBytes(bytes.toList())
                                if (found.isNotEmpty()) {
                                    results.value = found
                                }
                                proxy.close()
                            }
                            try {
                                provider.unbindAll()
                                provider.bindToLifecycle(
                                    ctx as androidx.lifecycle.LifecycleOwner,
                                    CameraSelector.DEFAULT_BACK_CAMERA,
                                    preview,
                                    analyzer,
                                )
                            } catch (_: Throwable) {}
                        }, ContextCompat.getMainExecutor(ctx))
                        previewView
                    },
                )
            }
        }
    }

    if (results.value.isNotEmpty()) {
        val vault = (repository.currentVault.value as? com.nerzhul.notp.data.VaultState.Unlocked)?.vault
        LaunchedEffect(results.value) {
            results.value.forEach { params ->
                vault?.addAccount(
                    issuer = params.issuer,
                    name = params.label,
                    secret = params.secret,
                    digits = params.digits,
                    period = params.period,
                    algorithm = params.algorithm,
                )
            }
            navController.popBackStack()
        }
    }
}
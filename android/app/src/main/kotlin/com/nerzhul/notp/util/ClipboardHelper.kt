package com.nerzhul.notp.util

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.PersistableBundle
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Copy [text] under [label] to the system clipboard. On Android 13+, the
 * system surfaces a thumbnail of the copied content; setting
 * [isSensitive] to true hides the preview from the OS UI. After
 * [clearAfterSeconds] seconds the clipboard is wiped (when supported by the
 * OS — Android 10+ only clears if our app wrote the entry).
 */
class ClipboardHelper(private val context: Context) {

    fun copy(
        text: String,
        label: String,
        isSensitive: Boolean = true,
        clearAfterSeconds: Long = 0,
        scope: CoroutineScope,
    ): Job {
        val manager = ContextCompat.getSystemService(context, ClipboardManager::class.java)
            ?: return scope.launch(Unit) { /* nothing to do */ }
        val clip = ClipData.newPlainText(label, text).apply {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && isSensitive) {
                description.extras = PersistableBundle().apply {
                    putBoolean(ClipboardDescription_IS_SENSITIVE, true)
                }
            }
        }
        manager.setPrimaryClip(clip)
        return if (clearAfterSeconds > 0 && Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            scope.launch(Dispatchers.Default) {
                delay(clearAfterSeconds * 1000)
                runCatching {
                    manager.clearPrimaryClip()
                }
            }
        } else if (clearAfterSeconds > 0) {
            scope.launch(Dispatchers.Default) {
                delay(clearAfterSeconds * 1000)
                // We can only reliably clear our own content; the system
                // ignores clearPrimaryClip() from third parties on older
                // OS versions, so just overwrite with an empty clip.
                runCatching {
                    manager.setPrimaryClip(ClipData.newPlainText(label, ""))
                }
            }
        } else {
            scope.launch(Unit) { /* noop */ }
        }
    }

    companion object {
        private const val ClipboardDescription_IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
    }
}
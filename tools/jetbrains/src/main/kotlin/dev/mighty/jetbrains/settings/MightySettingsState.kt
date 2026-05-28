// Persistent settings for the Mighty plugin.
//
// Stored as an application-level (i.e. IDE-wide, not per-project) component
// so users only configure the `mty` binary path once and every Mighty
// project picks it up.

package dev.mighty.jetbrains.settings

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage

@State(
    name = "dev.mighty.jetbrains.MightySettingsState",
    storages = [Storage("mighty.xml")],
)
class MightySettingsState : PersistentStateComponent<MightySettingsState> {
    /**
     * Absolute path to the `mty` binary. Empty means "let the OS resolve `mty`
     * on $PATH at process-spawn time", which is what we recommend by default.
     */
    var mtyBinaryPath: String = ""

    /**
     * Polling interval (seconds) for the Mighty Cost tool window. Defaulted
     * to 30s — the same cadence the mandate calls out.
     */
    var costPollSeconds: Int = 30

    /**
     * Whether the cost tool window should auto-refresh on open.
     */
    var costAutoRefresh: Boolean = true

    override fun getState(): MightySettingsState = this

    override fun loadState(state: MightySettingsState) {
        this.mtyBinaryPath = state.mtyBinaryPath
        this.costPollSeconds = state.costPollSeconds
        this.costAutoRefresh = state.costAutoRefresh
    }

    companion object {
        fun getInstance(): MightySettingsState =
            ApplicationManager.getApplication().getService(MightySettingsState::class.java)
    }
}

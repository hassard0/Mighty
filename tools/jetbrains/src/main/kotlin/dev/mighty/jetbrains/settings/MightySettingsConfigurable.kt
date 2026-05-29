// Settings UI: Settings > Tools > Mighty.
//
// Lets the user configure the `mty` binary path and cost-polling cadence
// without touching XML or environment variables.

package dev.mighty.jetbrains.settings

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.Configurable
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

class MightySettingsConfigurable : Configurable {
    private val state get() = MightySettingsState.getInstance()

    private val binaryPathField = TextFieldWithBrowseButton().apply {
        addBrowseFolderListener(
            "Locate `mty` Binary",
            "Pick the Mighty CLI binary that the plugin should launch as `<binary> lsp`.",
            null,
            FileChooserDescriptorFactory.createSingleFileNoJarsDescriptor(),
        )
    }

    private val pollSecondsField = JBTextField()
    private val autoRefreshCheckbox = JBCheckBox("Auto-refresh the Mighty Cost tool window")
    private val codeActionThresholdField = JBTextField()

    private var panel: JPanel? = null

    override fun getDisplayName(): String = "Mighty"

    override fun createComponent(): JComponent {
        val built = FormBuilder.createFormBuilder()
            .addLabeledComponent(JBLabel("`mty` binary path:"), binaryPathField, 1, false)
            .addTooltip("Empty = resolve `mty` on PATH at server-spawn time.")
            .addLabeledComponent(JBLabel("Cost poll interval (seconds):"), pollSecondsField, 1, false)
            .addComponent(autoRefreshCheckbox, 1)
            .addLabeledComponent(
                JBLabel("CodeAction confidence threshold (0.0..1.0):"),
                codeActionThresholdField,
                1,
                false,
            )
            .addTooltip(
                "Minimum fix-envelope confidence to surface as a Quick Fix on Alt+Enter. " +
                    "0.7 (default) hides speculative suggestions; lower to see more. " +
                    "Restart the LSP for changes to take effect.",
            )
            .addComponentFillVertically(JPanel(), 0)
            .panel
        panel = built
        return built
    }

    override fun isModified(): Boolean {
        return binaryPathField.text != state.mtyBinaryPath ||
            pollSecondsField.text.toIntOrNull() != state.costPollSeconds ||
            autoRefreshCheckbox.isSelected != state.costAutoRefresh ||
            codeActionThresholdField.text.toDoubleOrNull() != state.codeActionConfidenceThreshold
    }

    override fun apply() {
        state.mtyBinaryPath = binaryPathField.text.trim()
        state.costPollSeconds = pollSecondsField.text.toIntOrNull()?.coerceAtLeast(5) ?: 30
        state.costAutoRefresh = autoRefreshCheckbox.isSelected
        state.codeActionConfidenceThreshold =
            codeActionThresholdField.text.toDoubleOrNull()?.coerceIn(0.0, 1.0) ?: 0.7
    }

    override fun reset() {
        binaryPathField.text = state.mtyBinaryPath
        pollSecondsField.text = state.costPollSeconds.toString()
        autoRefreshCheckbox.isSelected = state.costAutoRefresh
        codeActionThresholdField.text = state.codeActionConfidenceThreshold.toString()
    }

    override fun disposeUIResources() {
        panel = null
    }
}

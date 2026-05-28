// v0.32 Track A — settings panel for the Mighty debug configuration.
//
// Pure Swing form so the editor stays simple + dependency-free. Each
// field maps 1:1 onto a `MightyDebugRunConfigurationOptions` slot via
// `applyEditorTo` / `resetEditorFrom`.

package dev.mighty.jetbrains.debug

import com.intellij.openapi.options.SettingsEditor
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBTextArea
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

class MightyDebugSettingsEditor : SettingsEditor<MightyDebugRunConfiguration>() {
    private val programField = JBTextField()
    private val replayTraceField = JBTextField()
    private val recordTraceField = JBTextField()
    private val argsArea = JBTextArea(3, 40)
    private val stopOnEntryBox = JBCheckBox("Stop on entry")

    private val panel: JPanel = FormBuilder.createFormBuilder()
        .addLabeledComponent(JBLabel("Program (.mty path):"), programField, 1, false)
        .addLabeledComponent(JBLabel("Replay trace (optional):"), replayTraceField, 1, false)
        .addLabeledComponent(JBLabel("Record trace (optional):"), recordTraceField, 1, false)
        .addLabeledComponent(JBLabel("Args (one per line):"), argsArea, 1, true)
        .addComponent(stopOnEntryBox, 1)
        .addComponentFillVertically(JPanel(), 0)
        .panel

    override fun resetEditorFrom(s: MightyDebugRunConfiguration) {
        programField.text = s.program
        replayTraceField.text = s.replayTrace
        recordTraceField.text = s.recordTrace
        argsArea.text = s.programArgs
        stopOnEntryBox.isSelected = s.stopOnEntry
    }

    override fun applyEditorTo(s: MightyDebugRunConfiguration) {
        s.program = programField.text.trim()
        s.replayTrace = replayTraceField.text.trim()
        s.recordTrace = recordTraceField.text.trim()
        s.programArgs = argsArea.text
        s.stopOnEntry = stopOnEntryBox.isSelected
    }

    override fun createEditor(): JComponent = panel
}

// Cost dashboard tool window: anchored bottom-right ("secondary=true" in
// plugin.xml). Polls `mty inspect --cost --json` on a configurable cadence
// (default 30s) and renders the result in a tree table.
//
// The polling happens on a background task with progress feedback. We swap
// the UI in via `invokeLater` so the EDT stays responsive.

package dev.mighty.jetbrains.toolwindow

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.util.ExecUtil
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.content.ContentFactory
import com.intellij.util.concurrency.AppExecutorUtil
import dev.mighty.jetbrains.settings.MightySettingsState
import java.awt.BorderLayout
import java.awt.Dimension
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import javax.swing.BorderFactory
import javax.swing.BoxLayout
import javax.swing.JButton
import javax.swing.JPanel

class MightyCostToolWindowFactory : ToolWindowFactory {

    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = MightyCostPanel(project)
        val content = ContentFactory.getInstance().createContent(panel, "", false)
        Disposer.register(content, panel)
        toolWindow.contentManager.addContent(content)
    }

    override fun shouldBeAvailable(project: Project): Boolean = true
}

private class MightyCostPanel(private val project: Project) :
    JPanel(BorderLayout()), com.intellij.openapi.Disposable {

    private val output = JBLabel("<html><i>Run a Mighty project and open this tool window — costs will appear here.</i></html>")
    private val refreshButton = JButton("Refresh now")
    private val openSettingsButton = JButton("Settings…")
    private var pollTask: ScheduledFuture<*>? = null

    init {
        border = BorderFactory.createEmptyBorder(8, 8, 8, 8)

        val header = JPanel().apply {
            layout = BoxLayout(this, BoxLayout.X_AXIS)
            add(JBLabel("<html><b>Mighty Cost Dashboard</b> &nbsp; <i>mty inspect --cost --json</i></html>"))
            add(javax.swing.Box.createHorizontalGlue())
            add(refreshButton)
            add(javax.swing.Box.createHorizontalStrut(4))
            add(openSettingsButton)
        }
        add(header, BorderLayout.NORTH)

        val scroll = JBScrollPane(output).apply {
            preferredSize = Dimension(600, 200)
        }
        add(scroll, BorderLayout.CENTER)

        refreshButton.addActionListener { runPollOnce() }
        openSettingsButton.addActionListener {
            com.intellij.openapi.options.ShowSettingsUtil.getInstance()
                .showSettingsDialog(project, "Mighty")
        }

        val settings = MightySettingsState.getInstance()
        if (settings.costAutoRefresh) {
            schedulePolling(settings.costPollSeconds.toLong().coerceAtLeast(5))
        }
    }

    private fun schedulePolling(seconds: Long) {
        pollTask?.cancel(false)
        pollTask = AppExecutorUtil.getAppScheduledExecutorService().scheduleWithFixedDelay(
            { runPollOnce() },
            0,
            seconds,
            TimeUnit.SECONDS,
        )
    }

    private fun runPollOnce() {
        AppExecutorUtil.getAppExecutorService().submit {
            val rendered = try {
                val configured = MightySettingsState.getInstance().mtyBinaryPath.trim()
                val binary = configured.ifEmpty { "mty" }
                val output = ExecUtil.execAndGetOutput(
                    GeneralCommandLine(binary, "inspect", "--cost", "--json")
                        .withWorkDirectory(project.basePath),
                    5_000,
                )
                if (output.exitCode != 0) {
                    "<html><pre>(non-zero exit ${output.exitCode})\n${escape(output.stderr.take(2000))}</pre></html>"
                } else {
                    renderJson(output.stdout.trim())
                }
            } catch (t: Throwable) {
                LOG.info("mty inspect --cost --json failed: ${t.message}")
                "<html><i>Couldn't reach the <code>mty</code> CLI: ${escape(t.message ?: "unknown error")}</i></html>"
            }
            ApplicationManager.getApplication().invokeLater {
                output.text = rendered
            }
        }
    }

    private fun renderJson(stdout: String): String {
        if (stdout.isEmpty()) {
            return "<html><i>(no cost data yet — open a Mighty project)</i></html>"
        }
        // We don't ship a JSON parser dependency for this stub; just dump
        // the raw response in a monospaced block. v0.32 turns this into a
        // proper TreeTable with sorted columns.
        return "<html><pre>${escape(stdout.take(4000))}</pre></html>"
    }

    private fun escape(s: String): String =
        s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

    override fun dispose() {
        pollTask?.cancel(true)
        pollTask = null
    }

    companion object {
        private val LOG = Logger.getInstance(MightyCostPanel::class.java)
    }
}

// Cost dashboard tool window: anchored bottom-right ("secondary=true" in
// plugin.xml). Polls `mty inspect --cost --json` on a configurable cadence
// (default 30s) and renders the result in a sortable TreeTable.
//
// v0.32: parses the JSON response into rows and renders a real
// JBTable with columns Date / Provider:Model / Calls / Cost ($). Right-
// click "Copy as JSON" copies the raw JSON payload to the clipboard.
//
// The polling runs on a pooled executor; UI swap happens on the EDT.

package dev.mighty.jetbrains.toolwindow

import com.google.gson.JsonParser
import com.google.gson.JsonSyntaxException
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.util.ExecUtil
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionPopupMenu
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.ide.CopyPasteManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.content.ContentFactory
import com.intellij.ui.table.JBTable
import com.intellij.util.concurrency.AppExecutorUtil
import dev.mighty.jetbrains.settings.MightySettingsState
import java.awt.BorderLayout
import java.awt.Dimension
import java.awt.datatransfer.StringSelection
import java.awt.event.MouseAdapter
import java.awt.event.MouseEvent
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import javax.swing.BorderFactory
import javax.swing.BoxLayout
import javax.swing.JButton
import javax.swing.JPanel
import javax.swing.RowSorter
import javax.swing.SortOrder
import javax.swing.table.DefaultTableModel
import javax.swing.table.TableRowSorter

class MightyCostToolWindowFactory : ToolWindowFactory {

    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = MightyCostPanel(project)
        val content = ContentFactory.getInstance().createContent(panel, "", false)
        Disposer.register(content, panel)
        toolWindow.contentManager.addContent(content)
    }

    override fun shouldBeAvailable(project: Project): Boolean = true
}

/**
 * One row of the cost table.
 *
 * `date` is the day-bucket the cost row applies to; `providerModel` is the
 * concatenation `provider:model` (e.g. "anthropic:claude-opus-4-7"); the
 * remaining columns mirror the JSON shape emitted by `mty inspect --cost
 * --json`.
 */
internal data class CostRow(
    val date: String,
    val providerModel: String,
    val calls: Long,
    val costUsd: Double,
)

/**
 * Result of one cost-poll cycle. Kept at file scope (instead of nested
 * inside [MightyCostPanel]) so the companion-object's `parseRows` helper
 * can construct values without going through the outer class's qualified
 * name dance.
 */
internal sealed interface PollResult {
    data class Rows(val rows: List<CostRow>, val rawJson: String) : PollResult
    data class Empty(val rawJson: String) : PollResult
    data class Error(val message: String) : PollResult
}

private class MightyCostPanel(private val project: Project) :
    JPanel(BorderLayout()), com.intellij.openapi.Disposable {

    private val statusLabel = JBLabel(
        "<html><i>Run a Mighty project and open this tool window — costs will appear here.</i></html>",
    )
    private val refreshButton = JButton("Refresh now")
    private val openSettingsButton = JButton("Settings…")
    private val tableModel = object : DefaultTableModel(
        arrayOf<Any>("Date", "Provider:Model", "Calls", "Cost ($)"),
        0,
    ) {
        override fun isCellEditable(row: Int, column: Int): Boolean = false
        override fun getColumnClass(column: Int): Class<*> = when (column) {
            2 -> java.lang.Long::class.java
            3 -> java.lang.Double::class.java
            else -> String::class.java
        }
    }
    private val table = JBTable(tableModel).apply {
        autoCreateRowSorter = false
        setRowSorter(
            TableRowSorter(tableModel).apply {
                // Default sort: most recent date first, then by cost descending.
                sortKeys = listOf(
                    RowSorter.SortKey(0, SortOrder.DESCENDING),
                    RowSorter.SortKey(3, SortOrder.DESCENDING),
                )
            },
        )
    }

    /** Raw JSON we last received, exposed via the "Copy as JSON" action. */
    @Volatile
    private var lastRawJson: String = ""

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

        val centre = JPanel(BorderLayout()).apply {
            add(JBScrollPane(table).apply { preferredSize = Dimension(600, 240) }, BorderLayout.CENTER)
            add(statusLabel, BorderLayout.SOUTH)
        }
        add(centre, BorderLayout.CENTER)

        // Right-click on the table → "Copy as JSON".
        table.addMouseListener(object : MouseAdapter() {
            override fun mousePressed(e: MouseEvent) = maybeShowPopup(e)
            override fun mouseReleased(e: MouseEvent) = maybeShowPopup(e)

            private fun maybeShowPopup(e: MouseEvent) {
                if (!e.isPopupTrigger) return
                showContextMenu(e)
            }
        })

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

    private fun showContextMenu(e: MouseEvent) {
        val copyAction = object : AnAction("Copy as JSON") {
            override fun actionPerformed(event: AnActionEvent) {
                val payload = lastRawJson.ifEmpty { "{}" }
                CopyPasteManager.getInstance().setContents(StringSelection(payload))
            }
        }
        val group = DefaultActionGroup().apply { add(copyAction) }
        val popup: ActionPopupMenu = ActionManager.getInstance()
            .createActionPopupMenu("MightyCostTable", group)
        popup.component.show(e.component, e.x, e.y)
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
            val result: PollResult = try {
                val configured = MightySettingsState.getInstance().mtyBinaryPath.trim()
                val binary = configured.ifEmpty { "mty" }
                val output = ExecUtil.execAndGetOutput(
                    GeneralCommandLine(binary, "inspect", "--cost", "--json")
                        .withWorkDirectory(project.basePath),
                    5_000,
                )
                if (output.exitCode != 0) {
                    PollResult.Error("(non-zero exit ${output.exitCode}) ${output.stderr.take(500)}")
                } else {
                    parseRows(output.stdout.trim())
                }
            } catch (t: Throwable) {
                LOG.info("mty inspect --cost --json failed: ${t.message}")
                PollResult.Error("Couldn't reach the `mty` CLI: ${t.message ?: "unknown error"}")
            }
            ApplicationManager.getApplication().invokeLater {
                applyResult(result)
            }
        }
    }

    private fun applyResult(result: PollResult) {
        when (result) {
            is PollResult.Rows -> {
                lastRawJson = result.rawJson
                tableModel.rowCount = 0
                for (row in result.rows) {
                    tableModel.addRow(
                        arrayOf<Any>(row.date, row.providerModel, row.calls, row.costUsd),
                    )
                }
                val total = result.rows.sumOf { it.costUsd }
                statusLabel.text =
                    "<html><i>${result.rows.size} row(s) — total cost \$${"%.4f".format(total)}</i></html>"
            }

            is PollResult.Empty -> {
                lastRawJson = result.rawJson
                tableModel.rowCount = 0
                statusLabel.text = "<html><i>(no cost data yet — open a Mighty project)</i></html>"
            }

            is PollResult.Error -> {
                statusLabel.text =
                    "<html><i>${escapeHtml(result.message.take(500))}</i></html>"
            }
        }
    }

    private fun escapeHtml(s: String): String =
        s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

    override fun dispose() {
        pollTask?.cancel(true)
        pollTask = null
    }

    companion object {
        private val LOG = Logger.getInstance(MightyCostPanel::class.java)

        /**
         * Parse the `mty inspect --cost --json` output into [CostRow]s.
         *
         * The accepted shapes are intentionally permissive — the CLI's JSON
         * schema is still in flux, so we look for any combination of
         * `{ date, provider, model, calls, cost }` on each entry of either
         * the top-level array or `entries` / `rows` / `costs` arrays under
         * an object root.
         */
        internal fun parseRows(stdout: String): PollResult {
            if (stdout.isEmpty()) return PollResult.Empty("")
            val root = try {
                JsonParser.parseString(stdout)
            } catch (e: JsonSyntaxException) {
                return PollResult.Error("Malformed JSON from mty: ${e.message?.take(200) ?: "parse error"}")
            }

            val candidates = when {
                root.isJsonArray -> root.asJsonArray
                root.isJsonObject -> {
                    val obj = root.asJsonObject
                    listOf("entries", "rows", "costs", "items", "buckets")
                        .firstNotNullOfOrNull { key ->
                            obj.get(key)?.takeIf { it.isJsonArray }?.asJsonArray
                        }
                        ?: return PollResult.Empty(stdout)
                }
                else -> return PollResult.Empty(stdout)
            }

            val rows = mutableListOf<CostRow>()
            for (element in candidates) {
                if (!element.isJsonObject) continue
                val obj = element.asJsonObject
                val date = obj.get("date")?.asStringOrNull()
                    ?: obj.get("day")?.asStringOrNull()
                    ?: obj.get("bucket")?.asStringOrNull()
                    ?: ""
                val provider = obj.get("provider")?.asStringOrNull() ?: ""
                val model = obj.get("model")?.asStringOrNull() ?: ""
                val providerModel = when {
                    provider.isNotEmpty() && model.isNotEmpty() -> "$provider:$model"
                    provider.isNotEmpty() -> provider
                    model.isNotEmpty() -> model
                    else -> obj.get("name")?.asStringOrNull() ?: ""
                }
                val calls = obj.get("calls")?.asLongOrZero()
                    ?: obj.get("count")?.asLongOrZero()
                    ?: 0L
                val cost = obj.get("cost")?.asDoubleOrZero()
                    ?: obj.get("cost_usd")?.asDoubleOrZero()
                    ?: obj.get("usd")?.asDoubleOrZero()
                    ?: 0.0
                rows += CostRow(date, providerModel, calls, cost)
            }
            return if (rows.isEmpty()) PollResult.Empty(stdout) else PollResult.Rows(rows, stdout)
        }

        private fun com.google.gson.JsonElement.asStringOrNull(): String? =
            if (isJsonPrimitive && asJsonPrimitive.isString) asString else null

        private fun com.google.gson.JsonElement.asLongOrZero(): Long =
            if (isJsonPrimitive && asJsonPrimitive.isNumber) asLong else 0L

        private fun com.google.gson.JsonElement.asDoubleOrZero(): Double =
            if (isJsonPrimitive && asJsonPrimitive.isNumber) asDouble else 0.0
    }
}

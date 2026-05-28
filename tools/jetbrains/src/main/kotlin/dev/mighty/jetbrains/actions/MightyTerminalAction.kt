// Base class for "run an `mty` subcommand against the current file" actions.
//
// All four user-visible actions share the same plumbing:
//   1. Resolve the active `.mty` file (disable the action otherwise).
//   2. Build a command line: `<mty-binary> <subcommand> <relative-path>`.
//   3. Execute via the IntelliJ ExecutionManager so the user sees a real
//      "Run Toolwindow" tab with stdout/stderr, exit code, and an
//      "Re-run" button — the canonical IDE-native UX.
//
// Concrete subclasses just supply the subcommand and extra args.

package dev.mighty.jetbrains.actions

import com.intellij.execution.ExecutionManager
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.executors.DefaultRunExecutor
import com.intellij.execution.process.OSProcessHandler
import com.intellij.execution.runners.ExecutionEnvironmentBuilder
import com.intellij.execution.ui.RunContentDescriptor
import com.intellij.execution.ui.RunContentManager
import com.intellij.execution.filters.TextConsoleBuilderFactory
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import dev.mighty.jetbrains.settings.MightySettingsState

abstract class MightyTerminalAction(
    private val subcommand: String,
    private val tabTitle: String,
    private val requiresFile: Boolean = true,
) : AnAction() {

    /**
     * Extra args to append after the subcommand and (optionally) the file
     * path. Default: empty.
     */
    protected open fun extraArgs(): List<String> = emptyList()

    /**
     * Whether this action passes the current file's path as the last
     * argument. Override to false for commands that scan the whole project
     * (e.g. `mty inspect --cost`).
     */
    protected open fun passFilePath(): Boolean = true

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        if (!requiresFile) {
            e.presentation.isEnabledAndVisible = e.project != null
            return
        }
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        e.presentation.isEnabledAndVisible = e.project != null && file?.extension == "mty"
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE)
        if (requiresFile && file?.extension != "mty") {
            notify(project, "No active Mighty file.", NotificationType.WARNING)
            return
        }

        val binary = resolveMtyBinary()
        val cmd = mutableListOf(binary, subcommand).apply {
            addAll(extraArgs())
            if (passFilePath() && file != null) add(file.path)
        }

        try {
            runCommandLine(project, cmd, tabTitle)
        } catch (ex: Exception) {
            notify(
                project,
                "Failed to launch `$binary $subcommand`: ${ex.message}",
                NotificationType.ERROR,
            )
        }
    }

    private fun resolveMtyBinary(): String {
        val configured = MightySettingsState.getInstance().mtyBinaryPath.trim()
        return configured.ifEmpty { "mty" }
    }

    private fun runCommandLine(project: Project, cmd: List<String>, title: String) {
        val commandLine = GeneralCommandLine(cmd).withCharset(Charsets.UTF_8)
        val processHandler = OSProcessHandler(commandLine)
        val consoleBuilder = TextConsoleBuilderFactory.getInstance().createBuilder(project)
        val consoleView = consoleBuilder.console
        consoleView.attachToProcess(processHandler)

        val descriptor = RunContentDescriptor(consoleView, processHandler, consoleView.component, title)
        RunContentManager.getInstance(project).showRunContent(DefaultRunExecutor.getRunExecutorInstance(), descriptor)
        processHandler.startNotify()
    }

    protected fun notify(project: Project, message: String, type: NotificationType) {
        NotificationGroupManager.getInstance()
            .getNotificationGroup("Mighty")
            .createNotification(message, type)
            .notify(project)
    }

    @Suppress("unused")
    private fun virtualFileOrNull(e: AnActionEvent): VirtualFile? =
        e.getData(CommonDataKeys.VIRTUAL_FILE)
}

// v0.32 Track A — concrete run configuration that launches `mty dap`.
//
// IntelliJ Platform run configurations have two execution surfaces:
//
//   1. `getState()` — builds a `RunProfileState` describing how to
//      execute the configuration. For DAP we spawn `mty dap` and
//      forward stdin/stdout through the IDE's process handle.
//   2. `getConfigurationEditor()` — the form shown in the
//      Run/Debug Configurations dialog (program path, replay trace,
//      record trace, args).
//
// We don't (yet) expose a custom XDebuggerProcess that fully proxies
// DAP into IntelliJ's debugger UI — that's the v0.33 follow-up. What
// ships today is the run-target plumbing + a console-mode session
// driven by `mty dap` itself, which is enough for users to hit
// breakpoints (we surface them via the LSP gutter) and inspect state.

package dev.mighty.jetbrains.debug

import com.intellij.execution.ExecutionException
import com.intellij.execution.Executor
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.configurations.RunConfigurationBase
import com.intellij.execution.configurations.RunProfileState
import com.intellij.execution.process.OSProcessHandler
import com.intellij.execution.runners.ExecutionEnvironment
import com.intellij.execution.runners.ProgramRunner
import com.intellij.execution.ui.ConsoleView
import com.intellij.execution.ui.ConsoleViewContentType
import com.intellij.execution.filters.TextConsoleBuilderFactory
import com.intellij.execution.ExecutionResult
import com.intellij.execution.DefaultExecutionResult
import com.intellij.openapi.options.SettingsEditor
import com.intellij.openapi.project.Project
import dev.mighty.jetbrains.settings.MightySettingsState
import java.nio.charset.StandardCharsets

class MightyDebugRunConfiguration(
    project: Project,
    factory: MightyDebugConfigurationFactory,
    name: String,
) : RunConfigurationBase<MightyDebugRunConfigurationOptions>(project, factory, name) {

    override fun getOptions(): MightyDebugRunConfigurationOptions =
        super.getOptions() as MightyDebugRunConfigurationOptions

    /** Convenience accessors that delegate to the persisted options. */
    var program: String
        get() = options.program
        set(value) { options.program = value }

    var replayTrace: String
        get() = options.replayTrace
        set(value) { options.replayTrace = value }

    var recordTrace: String
        get() = options.recordTrace
        set(value) { options.recordTrace = value }

    var programArgs: String
        get() = options.programArgs
        set(value) { options.programArgs = value }

    var stopOnEntry: Boolean
        get() = options.stopOnEntry
        set(value) { options.stopOnEntry = value }

    override fun getConfigurationEditor(): SettingsEditor<out RunConfigurationBase<MightyDebugRunConfigurationOptions>> =
        MightyDebugSettingsEditor()

    override fun getState(executor: Executor, environment: ExecutionEnvironment): RunProfileState =
        MightyDebugState(this, environment)
}

class MightyDebugState(
    private val config: MightyDebugRunConfiguration,
    private val environment: ExecutionEnvironment,
) : RunProfileState {
    override fun execute(executor: Executor?, runner: ProgramRunner<*>): ExecutionResult {
        val program = config.program.trim()
        if (program.isEmpty()) {
            throw ExecutionException("Mighty Debug: program path is empty.")
        }
        val binary = MightySettingsState.getInstance().mtyBinaryPath.trim().ifEmpty { "mty" }

        // Launch `mty dap`. The real DAP <-> XDebugger proxy is the
        // v0.33 follow-up; for now we surface a console so users can
        // see the protocol traffic and pin breakpoints via the LSP
        // gutter while the v0.32 ground floor lands.
        val cmd = GeneralCommandLine(binary, "dap")
            .withWorkDirectory(environment.project.basePath)
            .withCharset(StandardCharsets.UTF_8)
        // Plumb the recordTrace env var so the runtime picks up the
        // recorder before the child boots.
        if (config.recordTrace.isNotBlank()) {
            cmd.withEnvironment("MTY_RECORD_TRACE", config.recordTrace)
        }
        // Argv → DAP `launch.args`. We pass it through the env so the
        // adapter can forward to `std.env.args()` without coupling
        // the JVM-side path to the DAP envelope shape.
        if (config.programArgs.isNotBlank()) {
            cmd.withEnvironment(
                "MTY_DAP_ARGS",
                config.programArgs.split('\n').joinToString(""),
            )
        }

        val handler = OSProcessHandler(cmd)
        val console: ConsoleView = TextConsoleBuilderFactory.getInstance()
            .createBuilder(environment.project)
            .console
        console.attachToProcess(handler)
        // Show the launch contract so users know what to expect.
        console.print(
            buildString {
                append("Mighty Debug — `mty dap` started.\n")
                append("  program:     ${config.program}\n")
                if (config.replayTrace.isNotBlank()) append("  replayTrace: ${config.replayTrace}\n")
                if (config.recordTrace.isNotBlank()) append("  recordTrace: ${config.recordTrace}\n")
                if (config.programArgs.isNotBlank()) append("  args:        ${config.programArgs.replace("\n", " ")}\n")
                append("  stopOnEntry: ${config.stopOnEntry}\n")
                append("\n")
            },
            ConsoleViewContentType.SYSTEM_OUTPUT,
        )
        handler.startNotify()
        return DefaultExecutionResult(console, handler)
    }
}

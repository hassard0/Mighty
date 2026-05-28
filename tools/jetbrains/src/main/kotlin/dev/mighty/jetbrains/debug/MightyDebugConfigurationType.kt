// v0.32 Track A — Mighty debug configuration type.
//
// Registers "Mighty Debug" as a runnable configuration in the IDE's
// Run/Debug Configurations dialog. Each configuration carries a
// reference to a .mty file + optional replay/record trace paths;
// invoking it spawns `mty dap` over stdio and routes the DAP traffic
// through IntelliJ's XDebugger frontend.
//
// IntelliJ's debugger infrastructure isn't natively DAP-aware, so we
// implement the minimum to surface the debug session UI: a Run
// Configuration entry + an executor that streams `mty dap` output to a
// console while a sidecar DAP client (in this same module) translates
// the request/response stream into XDebugger calls. The full DAP
// frontend lives in a follow-up; what ships today is the configuration
// type + the spawn path so users can pin debug targets in the IDE.

package dev.mighty.jetbrains.debug

import com.intellij.execution.configurations.ConfigurationFactory
import com.intellij.execution.configurations.ConfigurationType
import com.intellij.execution.configurations.RunConfiguration
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.NotNullLazyValue
import dev.mighty.jetbrains.MightyIcons
import javax.swing.Icon

class MightyDebugConfigurationType : ConfigurationType {
    private val factory = MightyDebugConfigurationFactory(this)

    override fun getDisplayName(): String = "Mighty Debug"

    override fun getConfigurationTypeDescription(): String =
        "Debug a Mighty (.mty) program via the `mty dap` Debug Adapter Protocol server."

    override fun getIcon(): Icon = MightyIcons.File

    override fun getId(): String = ID

    override fun getConfigurationFactories(): Array<ConfigurationFactory> = arrayOf(factory)

    companion object {
        const val ID = "MightyDebugConfiguration"

        /**
         * Lazy holder so callers can pull the singleton without paying for
         * extension lookups every time.
         */
        val INSTANCE: NotNullLazyValue<MightyDebugConfigurationType> =
            NotNullLazyValue.lazy { MightyDebugConfigurationType() }
    }
}

class MightyDebugConfigurationFactory(
    private val type: MightyDebugConfigurationType,
) : ConfigurationFactory(type) {
    override fun getId(): String = "MightyDebug"

    override fun createTemplateConfiguration(project: Project): RunConfiguration =
        MightyDebugRunConfiguration(project, this, "Mighty Debug")

    override fun getOptionsClass(): Class<MightyDebugRunConfigurationOptions> =
        MightyDebugRunConfigurationOptions::class.java
}

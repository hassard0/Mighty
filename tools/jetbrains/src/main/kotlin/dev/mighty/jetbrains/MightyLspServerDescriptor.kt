// Describes the Mighty language server process.
//
// We launch `<mty> lsp` over stdio. The binary is resolved in three steps:
//   1. The path configured in Settings > Tools > Mighty (if non-empty).
//   2. The literal command `mty` — relying on the user's PATH.
//   3. Falls back to a no-op descriptor that logs a warning if no binary can
//      be located (so the IDE doesn't repeatedly try to spawn a missing
//      executable).
//
// The descriptor's `isSupportedFile` filter restricts the server to .mty
// files; the platform automatically routes hover/definition/completion/etc.
// to it once a file is opened.

package dev.mighty.jetbrains

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import dev.mighty.jetbrains.settings.MightySettingsState

class MightyLspServerDescriptor(project: Project) :
    ProjectWideLspServerDescriptor(project, "Mighty") {

    override fun isSupportedFile(file: VirtualFile): Boolean =
        file.extension == "mty"

    override fun createCommandLine(): GeneralCommandLine {
        val configured = MightySettingsState.getInstance().mtyBinaryPath.trim()
        val command = configured.ifEmpty { "mty" }
        return GeneralCommandLine(command, "lsp")
            .withCharset(Charsets.UTF_8)
            // The LSP API multiplexes stdin/stdout for us, so we just hand
            // back the command line and let the platform plumb the streams.
            .withRedirectErrorStream(false)
    }
}

// Wires Mighty .mty files into the IntelliJ Platform LSP API.
//
// The platform calls `fileOpened` for every editor opened in a project.
// We hand back a server descriptor only for files whose extension matches,
// and the platform then manages process lifecycle, request routing, and
// editor feature wiring (diagnostics, hover, completion, rename, etc.) on
// our behalf.
//
// Requires IntelliJ Platform >= 232 (2023.2), per plugin.xml's since-build.

package dev.mighty.jetbrains

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider

class MightyLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(
        project: Project,
        file: VirtualFile,
        serverStarter: LspServerSupportProvider.LspServerStarter,
    ) {
        if (file.extension != "mty") return
        serverStarter.ensureServerStarted(MightyLspServerDescriptor(project))
    }
}

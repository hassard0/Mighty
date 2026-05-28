// Run `mty test --eval` for the current file (or whole project, when invoked
// outside a Mighty file).

package dev.mighty.jetbrains.actions

class TestEvalAction : MightyTerminalAction(
    subcommand = "test",
    tabTitle = "mty test --eval",
    requiresFile = false,
) {
    override fun extraArgs(): List<String> = listOf("--eval")

    // Test runner discovers suites by walking the project tree itself.
    override fun passFilePath(): Boolean = false
}

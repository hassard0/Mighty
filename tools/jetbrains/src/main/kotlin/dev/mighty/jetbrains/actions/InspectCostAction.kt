// Run `mty inspect --cost` against the project (or the active file if one is
// open). The Mighty Cost tool window consumes the JSON variant of this same
// command separately; this action is for ad-hoc human-readable output.

package dev.mighty.jetbrains.actions

class InspectCostAction : MightyTerminalAction(
    subcommand = "inspect",
    tabTitle = "mty inspect --cost",
    requiresFile = false,
) {
    override fun extraArgs(): List<String> = listOf("--cost")

    // `mty inspect` walks the whole project root; no file path argument
    // needed.
    override fun passFilePath(): Boolean = false
}

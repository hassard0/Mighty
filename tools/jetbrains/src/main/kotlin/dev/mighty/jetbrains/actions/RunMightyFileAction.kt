// Run the active .mty file via `mty run`.

package dev.mighty.jetbrains.actions

class RunMightyFileAction : MightyTerminalAction(
    subcommand = "run",
    tabTitle = "mty run",
)

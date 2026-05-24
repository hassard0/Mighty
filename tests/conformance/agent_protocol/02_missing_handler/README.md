# 02 missing_handler

Protocol Echo declares Ping + Bye; agent Echoer implements only
`on Ping`. The unimplemented `Bye` message must produce MT4032.
Spec §13.

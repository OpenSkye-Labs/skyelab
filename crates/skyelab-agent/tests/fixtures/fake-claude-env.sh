#!/bin/sh
# Stand-in for `claude`: writes its whole env to ./agent.env (the project dir), says so, then waits for stdin to close.
env > agent.env
printf '{"type":"fake_env_written"}\n'
while IFS= read -r line; do :; done

#!/bin/sh
# Stand-in for a `claude` that prints one ~17 MB line, then a normal one.
head -c 17000000 /dev/zero | tr '\0' a
printf '\n{"type":"after"}\n'
cat >/dev/null

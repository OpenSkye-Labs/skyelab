#!/bin/sh
# Stand-in for `claude`: reports the GitHub env it got, then waits for stdin to close.
printf '{"type":"fake_gh_env","gh_config_dir":"%s","has_gh_config_dir":"%s","gh_token":"%s","github_token":"%s"}\n' \
  "$GH_CONFIG_DIR" "${GH_CONFIG_DIR+set}" "$GH_TOKEN" "$GITHUB_TOKEN"
while IFS= read -r line; do :; done

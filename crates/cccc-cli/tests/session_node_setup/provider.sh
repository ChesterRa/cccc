#!/bin/sh
# Only version/configuration commands are allowed. No model/session command exists.
runtime=${0##*/}
printf '%s:%s\n' "$runtime" "$*" >> "$HOME/native-calls"
if [ "$1" = --version ]; then
  if [ "$runtime" = codex ]; then echo 'codex-cli 0.163.0'; else echo '2.1.306 (Claude Code)'; fi
  exit 0
fi
if [ "$1" = mcp ]; then
  [ ! -f "$HOME/fail-native" ] || exit 19
  printf 'configured\n' > "$HOME/$runtime/registration"
  exit 0
fi
if [ "$runtime" = claude ] && [ "$1" = plugin ]; then
  marketplace="$CCCC_HOME/integrations/local-sessions"
  if [ "$2 $3" = 'marketplace list' ]; then
    if [ -f "$HOME/marketplace-added" ]; then
      printf '[{"name":"cccc-local","source":"directory","path":"%s"}]\n' "$marketplace"
    else echo '[]'; fi
  elif [ "$2" = list ]; then
    if [ -f "$HOME/plugin-installed" ]; then
      printf '[{"id":"cccc-local-sessions@cccc-local","scope":"user","readFromFolder":"%s/plugins/cccc-local-sessions"}]\n' "$marketplace"
    else echo '[]'; fi
  elif [ "$2 $3" = 'marketplace add' ]; then
    : > "$HOME/marketplace-added"
    echo '{"outcome":"ok"}'
  elif [ "$2" = install ] || [ "$2" = enable ]; then
    : > "$HOME/plugin-installed"
    echo '{"outcome":"ok"}'
  else exit 91; fi
  exit 0
fi
exit 92

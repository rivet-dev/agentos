#!/usr/bin/env bash
# Readers pin one completed generation. Publishers never delete prior generations.
resolve_generation_prefix() {
  local input=$1 resolved marker
  resolved=$(readlink -e -- "$input") || {
    echo "ImmutablePrefixRequired: cannot resolve $input; build its owned generation first" >&2
    return 2
  }
  [[ -d "$resolved" && "${resolved##*/}" == *.generation.* && -f "$resolved/.agentos-generation" ]] || {
    echo "ImmutablePrefixRequired: $input is not an owned immutable generation; rebuild it with the generation publisher" >&2
    return 2
  }
  if ! IFS= read -r marker < "$resolved/.agentos-generation"; then
    echo "ImmutablePrefixRequired: unreadable or incomplete generation marker in $input; rebuild its owned generation" >&2
    return 2
  fi
  [[ "$marker" == agentos-immutable-prefix-v1 ]] || {
    echo "ImmutablePrefixRequired: invalid generation marker in $input; rebuild its owned generation" >&2
    return 2
  }
  printf '%s\n' "$resolved"
}

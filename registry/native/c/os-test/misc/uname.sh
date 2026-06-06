#!/bin/sh
# This script extracts the relevant platform information from uname(1) with
# some special hacks for platforms whose uname semantics are different. It
# handles platforms that produce long strings or platforms whose nightly builds
# contain a useful build date.
case `uname -s` in
# AIX somehow puts its major version in -v and its minor version in -r and then
# -m provides the machine name rather than the architecture.
AIX)
  echo "$(uname -s) $(uname -v).$(uname -r) $(uname -p)"
  ;;
# Darwin version numbers are separate from macOS versions. Incude both.
Darwin)
  echo $(sw_vers --productName) $(sw_vers --productVersion) $(uname -srm)
  ;;
# Haiku nightly builds have a long version string. Simplify it by extracting the
# hrev and the corresponding date of the build.
Haiku)
  if uname -r | grep -Eq development &&
     uname -v | grep -Eq '^hrev[0-9]+ ... [0-9]+ [0-9]+'; then
    version=$(uname -v | grep -Eo '^hrev[0-9]+ ... [0-9]+ [0-9]+')
    echo "$(uname -s) $version $(uname -m)"
  else
    uname -srm
  fi
  ;;
# SunOS might either be Solaris or OmniOS. The version numbering is complex. The
# SunOS version number is -r and and the Solaris version number is -v. The
# leading SunOS version number is omitted from the Solaris version number, e.g.
# SunOS 5.11 is Solaris 11. Solaris has long versions like 11.4.89.207.2.
# Illumos forked from Solaris and is pretending to be SunOS and supplies its
# own version in -v which contains the distribution name, possibly a build
# number, and some sort of hash. OmniOS has the build number and OpenIndiana
# does not.
SunOS)
  case `uname -v` in
  # If there is a build number in -v, extract and use it.
  *-r[0-9]*)
    version=$(uname -v | sed -E 's/^.*-(r[0-9]+).*/\1/')
    echo "$(uname -sr) $version $(uname -p)"
    ;;
  # If there is a hash in -v, extract and use it.
  *-[a-A.Ff0-9]*)
    version=$(uname -v | sed -E 's/^.*-(r[a-fA-F0-9]+).*/\1/')
    echo "$(uname -sr) $version $(uname -p)"
    ;;
  # Otherwise use the raw -v value.
  *)
    uname -srvp
    ;;
  esac
  ;;
# Sortix development releases contain a release date in -v.
Sortix)
  if uname -r | grep -Eq -- '-'; then
    uname -srvm
  else
    uname -srm
  fi
  ;;
*) uname -srm ;;
esac

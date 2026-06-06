#!/bin/sh
mkdir -p -- "$(dirname "$2")"
./$1 > $2 2>&1
CODE=$?
if [ ! -s $2 ] || [ 2 -le $CODE ]; then
  echo "exit: $CODE" >> $2
fi

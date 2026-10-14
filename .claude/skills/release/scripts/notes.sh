#!/bin/sh
# Print the Release notes of one version: the body of the message of the annotated tag vX.Y.Z.
# Usage: notes.sh X.Y.Z
set -eu

version=${1:?usage: notes.sh X.Y.Z}

git tag -l --format='%(contents:body)' "v$version" | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}'

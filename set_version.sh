#!/bin/bash
# Only use GITHUB_REF when it points at a tag (e.g. refs/tags/v1.2.3)
if [[ "${GITHUB_REF}" == refs/tags/* ]]; then
  VERSION=${GITHUB_REF#refs/tags/}
fi
if [ -z "${VERSION}" ]; then
  VERSION=$(git tag | sort -V | grep '^v' | tail -n1)
fi
VERSION=$(echo $VERSION | sed 's/[^0-9.]//g')
if [ -z "${VERSION}" ]; then
  # A release tag must give a version, untagged (manual) builds may skip it
  if [[ "${GITHUB_REF}" == refs/tags/* ]]; then
    echo "Tag ${GITHUB_REF#refs/tags/} does not contain a version" >&2
    exit 1
  fi
  echo "Could not determine version, leaving Cargo.toml as is"
  exit 0
fi
# perl instead of sed -i, which differs between GNU (linux) and BSD (macOS)
perl -pi -e "s/^version = .*/version = \"${VERSION}\"/" Cargo.toml

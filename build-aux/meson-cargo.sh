#!/usr/bin/env bash
set -ex

MESON_SOURCE_ROOT="$1"
MESON_BUILD_ROOT="$2"
PROFILE="$3"
PACKAGE="$4"
FILENAME="$5"
OUTPUT="$6"
RUST_TARGET="$7" # last argument, optional

export CARGO_TARGET_DIR="$MESON_BUILD_ROOT"/target
export CARGO_HOME="$MESON_BUILD_ROOT"/cargo-home

PROFILE_DIR=debug
ARGS=()

if [[ "$PROFILE" != "dev" ]]; then
    PROFILE_DIR="$PROFILE"
fi

if [[ -n "$RUST_TARGET" ]]; then
    ARGS+=( '--target' "$RUST_TARGET" )
fi


cargo build --profile "$PROFILE" "${ARGS[@]}" --manifest-path="$MESON_SOURCE_ROOT"/Cargo.toml -p "$PACKAGE"

if [[ -n "$RUST_TARGET" ]]; then
    cp "${CARGO_TARGET_DIR}/${RUST_TARGET}/${PROFILE_DIR}/${FILENAME}" "$OUTPUT"
else
    cp "${CARGO_TARGET_DIR}/${PROFILE_DIR}/${FILENAME}" "$OUTPUT"
fi

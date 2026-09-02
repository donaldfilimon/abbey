#!/bin/sh
# Fetch one allowlisted public sibling at an immutable revision without using
# the workflow token or any credential helper from a self-hosted runner.
set -eu

if [ "$#" -ne 3 ]; then
    echo "usage: checkout-public-revision.sh <owner/repo> <40-hex-revision> <destination>" >&2
    exit 2
fi

repository=$1
revision=$2
destination=$3

case "$repository:$destination" in
    donaldfilimon/abi:abi | donaldfilimon/wdbx:wdbx) ;;
    *)
        echo "public-checkout: repository/destination pair is not allowlisted" >&2
        exit 2
        ;;
esac

case "$revision" in
    "" | *[!0-9a-f]*)
        echo "public-checkout: revision must be exactly 40 lowercase hexadecimal characters" >&2
        exit 2
        ;;
esac
if [ "${#revision}" -ne 40 ]; then
    echo "public-checkout: revision must be exactly 40 lowercase hexadecimal characters" >&2
    exit 2
fi

workspace=${GITHUB_WORKSPACE:?public-checkout requires GITHUB_WORKSPACE}
case "$workspace" in
    /*) ;;
    *)
        echo "public-checkout: GITHUB_WORKSPACE must be an absolute path" >&2
        exit 2
        ;;
esac

checkout_path="$workspace/$destination"
case "$checkout_path" in
    "$workspace/abi" | "$workspace/wdbx") ;;
    *)
        echo "public-checkout: destination escaped the allowlisted workspace paths" >&2
        exit 2
        ;;
esac

if [ -e "$checkout_path" ] || [ -L "$checkout_path" ]; then
    # `find` does not follow a destination symlink by default. This removes
    # only the exact allowlisted checkout path, including stale runner state.
    find "$checkout_path" -depth -delete
fi
mkdir -p "$checkout_path"

# Do not let a self-hosted runner's global/system Git configuration, keychain
# helper, or interactive prompt turn this public proof into a credentialed
# checkout. A private repository must fail closed here.
export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_TERMINAL_PROMPT=0
export GCM_INTERACTIVE=never
export GIT_ASKPASS=/bin/false
export SSH_ASKPASS=/bin/false

git -C "$checkout_path" init --quiet
git -C "$checkout_path" remote add origin "https://github.com/$repository.git"
git -C "$checkout_path" -c credential.helper= fetch \
    --quiet --no-tags --depth=1 origin "$revision"
git -C "$checkout_path" checkout --quiet --detach --force FETCH_HEAD

actual_revision=$(git -C "$checkout_path" rev-parse HEAD)
if [ "$actual_revision" != "$revision" ]; then
    echo "public-checkout: fetched revision does not match the immutable pin" >&2
    exit 1
fi

echo "public-checkout: $repository@$actual_revision"

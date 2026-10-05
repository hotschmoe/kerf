#!/usr/bin/env bash
# usage: test/shot.sh <port> <out.png> "<query>" [extra shot.mjs args]   (serves nothing; expects a server on <port>)
port=$1; out=$2; q=$3; shift 3
node "$(dirname "$0")/../../../tools/shot.mjs" "http://localhost:$port/?$q" "$out" --wait-for "window.__ready===true" --wait-ms 700 "$@"

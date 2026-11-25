#!/usr/bin/env bash
set -euo pipefail

mkdir -p target/distrib
install -m 755 packaging/install-linux.sh target/distrib/kuizer-gui-linux-installer.sh

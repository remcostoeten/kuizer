# Paths Kuizer protects

Kuizer never removes an item merely because a scan found it. The deletion guard checks each selected item again before removal.

Kuizer refuses system paths, the home directory itself, mount points, scan roots and paths outside a scan root. It also refuses configured protected paths. Configuration can add protected paths, but cannot remove built-in protections.

The scanner and duplicate finder avoid browser profiles and application data, including Chrome, Chromium, Brave, Firefox, Edge and other known browser profiles. They also protect SSH and GnuPG data, password stores, keyrings, globally installed tools, and editor extensions. Detection checks known paths and markers inside directories so renamed profile folders remain protected.

Scans do not follow symlinks. They skip excluded mounts and filesystem types, and they stay on the starting filesystem by default. Deletion revalidates the scanned item and refuses symlink replacements and changed identities. A selected path must remain within its scan root and must pass the built-in protection checks.

Kuizer does not remove duplicate files. Duplicate reports are informational, and you choose which copy to keep.

For source-specific removal behavior and trash rules, see [What each source removes](removal-sources.md).

#!/usr/bin/env python3
"""Split a nextest JUnit report into one file per Codecov crate flag.

Usage: tools/split-junit-flags.py <junit.xml> <output-dir>

Codecov's flag filter on the Tests dashboard is set per *upload*, not per `<testsuite>` inside one
upload. A single combined-workspace upload can therefore carry at most one flag without
misattributing every crate's tests to every other crate's flag. So the report is split first and
each piece uploaded under its own flag.

nextest names each `<testsuite>` after the Cargo target it came from: `turnbase`, `turnbase-bots`,
`turnbase-session`, and so on, with integration-test binaries suffixed (`coup::dashboard_smoke`).
Anything that is not one of the seven publishable crates -- the example games and their integration
tests -- goes to `unflagged`, which uploads with no flag rather than being dropped.
"""

import sys
import os
import xml.etree.ElementTree as ET

# Cargo package name -> Codecov flag. The flag is the crate's directory under crates/, which is what
# codecov.yml's `flags:` and `component_management:` entries are keyed on.
CRATE_FLAGS = {
    "turnbase": "core",
    "turnbase-bots": "bots",
    "turnbase-match": "match",
    "turnbase-simulator": "simulator",
    "turnbase-protocol": "protocol",
    "turnbase-session": "session",
    "turnbase-cli": "cli",
}
UNFLAGGED = "unflagged"


def flag_for(suite_name: str) -> str:
    """Map a testsuite name to a flag, stripping any `::binary` suffix first."""
    return CRATE_FLAGS.get(suite_name.split("::", 1)[0], UNFLAGGED)


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    src, outdir = sys.argv[1], sys.argv[2]

    if not os.path.exists(src):
        print(f"error: {src} does not exist", file=sys.stderr)
        return 1

    root = ET.parse(src).getroot()
    os.makedirs(outdir, exist_ok=True)

    buckets: dict[str, list[ET.Element]] = {}
    for suite in root:
        buckets.setdefault(flag_for(suite.get("name", "")), []).append(suite)

    # Always emit a file for every known flag, even when empty. An absent file would make the
    # upload step fail for a crate that happens to have no tests in this run, which is a worse
    # failure mode than an empty report.
    for flag in list(CRATE_FLAGS.values()) + [UNFLAGGED]:
        suites = buckets.get(flag, [])
        out = ET.Element("testsuites")
        totals = {"tests": 0, "failures": 0, "errors": 0}
        for suite in suites:
            out.append(suite)
            for key in totals:
                totals[key] += int(suite.get(key) or 0)
        for key, value in totals.items():
            out.set(key, str(value))

        path = os.path.join(outdir, f"{flag}.xml")
        ET.ElementTree(out).write(path, encoding="utf-8", xml_declaration=True)
        print(f"{path}: {len(suites)} suite(s), {totals['tests']} test(s)")

    return 0


if __name__ == "__main__":
    sys.exit(main())

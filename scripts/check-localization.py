#!/usr/bin/env python3
"""Check that every localizable string in the macOS app has a translation.

SwiftUI turns string literals in Text, Label, Button, ... and String(localized:)
into localization keys; interpolations become format specifiers. This script
extracts those keys (assuming every interpolated value is a String, i.e. `%@`,
which is the convention in this codebase) and compares them with each
<lang>.lproj/Localizable.strings.

Usage:
    scripts/check-localization.py            # report missing / unused keys
    scripts/check-localization.py --keys     # print all keys (for new languages)
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCES = ROOT / "apps/macos/Sources/Miiting"
RESOURCES = ROOT / "apps/macos/Resources"

# Call sites whose first string literal argument is a LocalizedStringKey.
CALLS = [
    "Text", "Label", "Button", "Toggle", "Picker", "Section", "TextField", "SecureField", "Stepper",
    "Menu", "LabeledContent", "Tab", "ContentUnavailableView", "Link",
    # modifiers
    r"\.help", r"\.navigationTitle", r"\.alert", r"\.confirmationDialog", r"\.accessibilityLabel",
    # helpers in this codebase that take LocalizedStringKey
    "point", "tag", "flow", "modelPicker",
]
LITERAL = r'"((?:[^"\\]|\\.)*)"'
SKIP_PREFIX = re.compile(r"(?:Text|Label|Button)\(verbatim:")


def interpolation_to_specifier(s: str) -> str:
    """Replace \\( ... ) (with nested parentheses) by %@."""
    out, i = [], 0
    while i < len(s):
        if s.startswith("\\(", i):
            depth, j = 1, i + 2
            while j < len(s) and depth:
                if s[j] == "(":
                    depth += 1
                elif s[j] == ")":
                    depth -= 1
                j += 1
            out.append("%@")
            i = j
        else:
            out.append(s[i])
            i += 1
    return "".join(out)


def extract_line_literal(line: str, start: int) -> str:
    """Read a Swift string literal starting at `start` (the opening quote), honoring interpolation."""
    i, depth, out = start + 1, 0, []
    while i < len(line):
        c = line[i]
        if depth == 0 and c == '"':
            break
        if line.startswith("\\(", i):
            depth += 1
            out.append("\\(")
            i += 2
            continue
        if depth and c == "(":
            depth += 1
        elif depth and c == ")":
            depth -= 1
        out.append(c)
        i += 1
    return "".join(out)


def keys_in_source() -> dict:
    keys = {}
    calls = [c for c in CALLS if c != "point"]
    call_re = re.compile(r"(?:String\(localized:\s*|prompt:\s*|(?<![\w.])(?:" + "|".join(calls) + r")\(\s*)\"")
    for path in sorted(SOURCES.rglob("*.swift")):
        for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("//") or SKIP_PREFIX.search(line):
                continue
            for m in call_re.finditer(line):
                literal = extract_line_literal(line, m.end() - 1)
                key = interpolation_to_specifier(literal)
                if key and not re.fullmatch(r"[a-z0-9.\-_]+", key):
                    keys.setdefault(key, f"{path.relative_to(ROOT)}:{n}")
            if re.search(r"(?<![\w.])point\(", line):
                lits = re.findall(LITERAL, line)
                for lit in lits[1:3]:  # point("icon", "Title", "Text")
                    keys.setdefault(interpolation_to_specifier(lit), f"{path.relative_to(ROOT)}:{n}")
    return keys


def strings_file(path: pathlib.Path) -> dict:
    entries = {}
    text = path.read_text(encoding="utf-8")
    for m in re.finditer(r'^\s*"((?:[^"\\]|\\.)*)"\s*=\s*"((?:[^"\\]|\\.)*)"\s*;', text, re.M):
        entries[m.group(1)] = m.group(2)
    return entries


def main() -> int:
    keys = keys_in_source()
    if "--keys" in sys.argv:
        for k in sorted(keys):
            print(k)
        return 0
    status = 0
    for lproj in sorted(RESOURCES.glob("*.lproj")):
        table = lproj / "Localizable.strings"
        if lproj.name == "en.lproj" or not table.exists():
            continue
        entries = strings_file(table)
        missing = [k for k in keys if k not in entries]
        unused = [k for k in entries if k not in keys]
        for k in missing:
            print(f"{lproj.name}: missing  \"{k}\"  ({keys[k]})")
        for k in unused:
            print(f"{lproj.name}: unused   \"{k}\"")
        bad = [k for k, v in entries.items() if k.count("%@") != len(re.findall(r"%(?:\d+\$)?@", v))]
        for k in bad:
            print(f"{lproj.name}: placeholder count differs for \"{k}\"")
        print(f"{lproj.name}: {len(keys) - len(missing)}/{len(keys)} translated, {len(unused)} unused, {len(bad)} mismatched")
        status |= bool(missing or bad)
    return status


if __name__ == "__main__":
    sys.exit(main())

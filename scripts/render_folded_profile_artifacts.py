#!/usr/bin/env python3
import argparse
import shutil
import subprocess
from pathlib import Path


def write_tree(folded_path: Path, tree_path: Path) -> None:
    tree: dict[str, dict] = {}
    with folded_path.open(encoding="utf-8") as handle:
        for raw in handle:
            raw = raw.strip()
            if not raw:
                continue
            stack, count_text = raw.rsplit(" ", 1)
            count = int(count_text)
            node = tree
            node["_total"] = node.get("_total", 0) + count
            for frame in stack.split(";"):
                node = node.setdefault(frame, {})
                node["_total"] = node.get("_total", 0) + count

    lines: list[str] = []

    def walk(node: dict, total: int, indent: str) -> None:
        children = [
            (name, child["_total"])
            for name, child in node.items()
            if name != "_total"
        ]
        children.sort(key=lambda item: item[1], reverse=True)
        for name, value in children:
            lines.append(f"{value / total * 100:6.2f}% {indent}{name}")
            walk(node[name], total, indent + "  ")

    walk(tree, tree["_total"], "")
    tree_path.write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("folded")
    parser.add_argument("--output-prefix", required=True)
    args = parser.parse_args()

    if shutil.which("inferno-flamegraph") is None:
        raise SystemExit("missing required tool: inferno-flamegraph")

    folded_path = Path(args.folded)
    prefix = Path(args.output_prefix)
    prefix.parent.mkdir(parents=True, exist_ok=True)

    svg_path = Path(f"{prefix}.svg")
    tree_path = Path(f"{prefix}.tree.txt")

    with folded_path.open("rb") as folded_in, svg_path.open("wb") as svg_out:
        subprocess.run(
            ["inferno-flamegraph"],
            check=True,
            stdin=folded_in,
            stdout=svg_out,
        )

    write_tree(folded_path, tree_path)

    print(svg_path)
    print(tree_path)


if __name__ == "__main__":
    main()

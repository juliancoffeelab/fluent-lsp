#!/usr/bin/env python3
import argparse
import shutil
import subprocess
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("time_profile_xml")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    if shutil.which("inferno-collapse-xctrace") is None:
        raise SystemExit("missing required tool: inferno-collapse-xctrace")
    if shutil.which("rustfilt") is None:
        raise SystemExit("missing required tool: rustfilt")

    xml_path = Path(args.time_profile_xml)
    folded_path = Path(args.output)
    folded_path.parent.mkdir(parents=True, exist_ok=True)

    with xml_path.open("rb") as xml_in, folded_path.open("wb") as folded_out:
        collapse = subprocess.Popen(
            ["inferno-collapse-xctrace"],
            stdin=xml_in,
            stdout=subprocess.PIPE,
        )
        assert collapse.stdout is not None
        rustfilt = subprocess.Popen(
            ["rustfilt"],
            stdin=collapse.stdout,
            stdout=folded_out,
        )
        collapse.stdout.close()
        rustfilt.wait()
        collapse.wait()
        if rustfilt.returncode != 0 or collapse.returncode != 0:
            raise SystemExit("failed to build folded stacks")

    print(folded_path)


if __name__ == "__main__":
    main()

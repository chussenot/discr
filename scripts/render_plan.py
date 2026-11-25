#!/usr/bin/env python3
"""Render plans/discr-loop.yaml for ONE bead.

Why this exists (found by running the loop for real on bernstein 4.0.0, after a
dry run had passed):

  * A plan cannot know which bead it is working on. The step text says "read
    the bead" and nothing tells an agent WHICH one.
  * `bernstein run` does NOT apply a plan's top-level `constraints:` -- its own
    source lists ("max_agents", "repos", "constraints") as settings "`bernstein
    run` does not apply", and `--from-plan` refuses a plan that sets one. The
    house rules in the plan never reached an agent. Step descriptions do reach
    agents as task text, so that is where the rules are put.
  * The agent has to be told which paths it holds leases on.

So: read the plan, take `constraints` OUT (leaving the key in would make the
file a plan the strict loader refuses), and prepend one context block -- the
bead, the leased paths, and every house rule -- to each step's description.

    render_plan.py PLAN OUT --bead discr-lul --title "..." --files "a b c"

Exits non-zero if the result would still contain a placeholder or a
`constraints:` key, so a bad render stops the loop before an agent is spent.
"""
import argparse
import sys

import yaml


def render(plan, bead, title, files):
    rules = plan.pop("constraints", None) or []

    block = [f"THIS RUN IS FOR BEAD {bead}: {title}"]
    if files:
        block.append(
            "You hold pact leases on exactly these paths, and may write ONLY "
            "these: " + ", ".join(files)
        )
    else:
        block.append(
            "No paths were leased for this run. Write nothing shared; report "
            "what you would change instead."
        )
    if rules:
        block.append("HOUSE RULES (every one applies to this step):")
        block.extend(f"- {r}" for r in rules)
    header = "\n".join(block)

    for stage in plan.get("stages") or []:
        for step in stage.get("steps") or []:
            body = str(step.get("description") or "").replace("<bead>", bead)
            step["description"] = f"{header}\n\n---\n\n{body.strip()}"
    return plan


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("plan")
    ap.add_argument("out")
    ap.add_argument("--bead", required=True)
    ap.add_argument("--title", required=True)
    ap.add_argument("--files", default="")
    a = ap.parse_args()

    with open(a.plan, encoding="utf-8") as fh:
        plan = yaml.safe_load(fh)
    if not isinstance(plan, dict) or not plan.get("stages"):
        sys.exit(f"render_plan: {a.plan} has no stages")

    plan = render(plan, a.bead, a.title, a.files.split())
    text = yaml.safe_dump(plan, sort_keys=False, allow_unicode=True, width=10_000)

    # Fail closed: a render that still carries one of these would either be
    # refused by `bernstein run` or silently drop what it was meant to carry.
    if "<bead>" in text:
        sys.exit("render_plan: an unreplaced <bead> placeholder survived")
    if "constraints" in plan:
        sys.exit("render_plan: constraints key survived (bernstein would not apply it)")

    with open(a.out, "w", encoding="utf-8") as fh:
        fh.write(text)


if __name__ == "__main__":
    main()
